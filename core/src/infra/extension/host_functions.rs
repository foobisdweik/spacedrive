//! WASM host functions
//!
//! This module provides the bridge between WASM extensions and Spacedrive's
//! operation registry. The key function is `host_spacedrive_call()` which routes
//! generic Wire method calls to the existing `execute_json_operation()` function
//! used by daemon RPC.

use std::sync::Arc;

use uuid::Uuid;
use wasmer::{FunctionEnvMut, Memory, MemoryView, WasmPtr};

use crate::{infra::daemon::rpc::RpcServer, Core};

use super::permissions::ExtensionPermissions;

/// Environment passed to all host functions
pub struct PluginEnv {
	pub extension_id: String,
	pub core_context: Arc<crate::context::CoreContext>, // Just context, not full Core!
	pub api_dispatcher: Arc<crate::infra::api::ApiDispatcher>, // For creating sessions
	pub permissions: ExtensionPermissions,
	pub memory: Memory,
	pub job_registry: Arc<super::job_registry::ExtensionJobRegistry>,
}

/// Route a generic extension `spacedrive_call` to the Wire operation registry.
///
/// This performs the same dispatch as `execute_json_operation()`: it tries the
/// library-query, core-query, library-action, then core-action registries in
/// order, applying library context to the session for library-scoped
/// operations, and returns the operation's JSON result (or an error string).
///
/// Extracted from [`host_spacedrive_call`] so the bridge routing can be
/// exercised end-to-end (real `CoreContext` + registered VDFS operation)
/// without instantiating a WASM guest and threading data through linear memory.
pub async fn dispatch_extension_call(
	core_context: Arc<crate::context::CoreContext>,
	api_dispatcher: Arc<crate::infra::api::ApiDispatcher>,
	method: &str,
	library_id: Option<Uuid>,
	payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
	let base_session = api_dispatcher.create_base_session()?;

	if let Some(handler) = crate::infra::wire::registry::LIBRARY_QUERIES.get(method) {
		let lib_id = library_id.ok_or_else(|| "Library ID required".to_string())?;
		let session = base_session.with_library(lib_id);
		return handler(core_context, session, payload).await;
	}

	if let Some(handler) = crate::infra::wire::registry::CORE_QUERIES.get(method) {
		return handler(core_context, base_session, payload).await;
	}

	if let Some(handler) = crate::infra::wire::registry::LIBRARY_ACTIONS.get(method) {
		let lib_id = library_id.ok_or_else(|| "Library ID required".to_string())?;
		let session = base_session.with_library(lib_id);
		return handler(core_context, session, payload).await;
	}

	if let Some(handler) = crate::infra::wire::registry::CORE_ACTIONS.get(method) {
		return handler(core_context, payload).await;
	}

	Err(format!("Unknown method: {}", method))
}

/// THE MAIN HOST FUNCTION - Generic Wire RPC
///
/// This is the ONLY function WASM extensions need to call Spacedrive operations.
/// It reads the call out of WASM linear memory, checks permissions, then routes
/// through [`dispatch_extension_call`] to the existing Wire operation registry.
///
/// # Arguments
/// - `method_ptr`, `method_len`: Wire method string (e.g., "query:ai.ocr")
/// - `library_id_ptr`: 0 for None, or pointer to 16 UUID bytes
/// - `payload_ptr`, `payload_len`: JSON payload string
///
/// # Returns
/// Pointer to result JSON string in WASM memory (or 0 on error)
pub fn host_spacedrive_call(
	mut env: FunctionEnvMut<PluginEnv>,
	method_ptr: WasmPtr<u8>,
	method_len: u32,
	library_id_ptr: u32,
	payload_ptr: WasmPtr<u8>,
	payload_len: u32,
) -> u32 {
	let (plugin_env, mut store) = env.data_and_store_mut();

	// Get memory view from environment
	let memory = &plugin_env.memory;
	let memory_view = memory.view(&store);

	// 1. Read method string from WASM memory
	let method = match read_string_from_wasm(&memory_view, method_ptr, method_len) {
		Ok(m) => m,
		Err(e) => {
			tracing::error!("Failed to read method string: {}", e);
			return 0;
		}
	};

	// 2. Read library_id (0 = None)
	let library_id = if library_id_ptr == 0 {
		None
	} else {
		match read_uuid_from_wasm(&memory_view, WasmPtr::new(library_id_ptr)) {
			Ok(uuid) => Some(uuid),
			Err(e) => {
				tracing::error!("Failed to read library UUID: {}", e);
				return 0;
			}
		}
	};

	// 3. Read payload JSON
	let payload_str = match read_string_from_wasm(&memory_view, payload_ptr, payload_len) {
		Ok(s) => s,
		Err(e) => {
			tracing::error!("Failed to read payload: {}", e);
			return 0;
		}
	};

	let payload_json: serde_json::Value = match serde_json::from_str(&payload_str) {
		Ok(json) => json,
		Err(e) => {
			tracing::error!("Failed to parse payload JSON: {}", e);
			return write_error_to_memory(&memory, &mut store, &format!("Invalid JSON: {}", e));
		}
	};

	// 4. Permission check
	let auth_result = tokio::runtime::Handle::current()
		.block_on(async { plugin_env.permissions.authorize(&method, library_id).await });

	if let Err(e) = auth_result {
		tracing::warn!(
			extension = %plugin_env.extension_id,
			method = %method,
			"Permission denied: {}",
			e
		);
		return write_error_to_memory(&memory, &mut store, &format!("Permission denied: {}", e));
	}

	tracing::debug!(
		extension = %plugin_env.extension_id,
		method = %method,
		library_id = ?library_id,
		"Extension calling operation"
	);

	// 5. Route to the Wire operation registry (same dispatch as
	//    execute_json_operation) via the shared, testable helper.
	let result = tokio::runtime::Handle::current().block_on(dispatch_extension_call(
		plugin_env.core_context.clone(),
		plugin_env.api_dispatcher.clone(),
		&method,
		library_id,
		payload_json,
	));

	// 6. Write result to WASM memory
	match result {
		Ok(json) => write_json_to_memory(&memory, &mut store, &json),
		Err(e) => {
			tracing::error!("Operation failed: {}", e);
			write_error_to_memory(&memory, &mut store, &e)
		}
	}
}

/// Optional logging helper for extensions
pub fn host_spacedrive_log(
	mut env: FunctionEnvMut<PluginEnv>,
	level: u32,
	msg_ptr: WasmPtr<u8>,
	msg_len: u32,
) {
	let (plugin_env, mut store) = env.data_and_store_mut();

	// Get memory view from environment
	let memory = &plugin_env.memory;
	let memory_view = memory.view(&store);

	let message = match read_string_from_wasm(&memory_view, msg_ptr, msg_len) {
		Ok(msg) => msg,
		Err(_) => {
			tracing::error!("Failed to read log message from WASM");
			return;
		}
	};

	match level {
		0 => tracing::debug!(extension = %plugin_env.extension_id, "{}", message),
		1 => tracing::info!(extension = %plugin_env.extension_id, "{}", message),
		2 => tracing::warn!(extension = %plugin_env.extension_id, "{}", message),
		3 => tracing::error!(extension = %plugin_env.extension_id, "{}", message),
		_ => tracing::info!(extension = %plugin_env.extension_id, "{}", message),
	}
}

// === Memory Helpers ===

fn read_string_from_wasm(
	memory_view: &MemoryView,
	ptr: WasmPtr<u8>,
	len: u32,
) -> Result<String, Box<dyn std::error::Error>> {
	let bytes = ptr
		.slice(memory_view, len)
		.and_then(|slice| slice.read_to_vec())
		.map_err(|e| format!("Failed to read from WASM memory: {:?}", e))?;

	String::from_utf8(bytes).map_err(|e| e.into())
}

fn read_uuid_from_wasm(
	memory_view: &MemoryView,
	ptr: WasmPtr<u8>,
) -> Result<Uuid, Box<dyn std::error::Error>> {
	let bytes = ptr
		.slice(memory_view, 16)
		.and_then(|slice| slice.read_to_vec())
		.map_err(|e| format!("Failed to read UUID from WASM memory: {:?}", e))?;

	let uuid_bytes: [u8; 16] = bytes
		.try_into()
		.map_err(|_| "Invalid UUID bytes (expected 16 bytes)")?;

	Ok(Uuid::from_bytes(uuid_bytes))
}

fn write_json_to_memory(
	memory: &Memory,
	store: &mut wasmer::StoreMut,
	json: &serde_json::Value,
) -> u32 {
	let json_str = match serde_json::to_string(json) {
		Ok(s) => s,
		Err(e) => {
			tracing::error!("Failed to serialize JSON: {}", e);
			return 0; // NULL indicates error
		}
	};

	let bytes = json_str.as_bytes();

	// Try to call guest's allocator function
	// WASM module must export: fn wasm_alloc(size: i32) -> i32
	let alloc_result = memory
		.view(&store)
		.data_size() // Just check memory exists for now
		.checked_sub(bytes.len() as u64);

	if alloc_result.is_none() {
		tracing::error!("Not enough WASM memory for result");
		return 0;
	}

	// For now, write to a fixed offset (will implement proper allocator later)
	// This is a simplification for testing - production needs guest allocator
	let result_offset = 65536u32; // Start at 64KB

	let memory_view = memory.view(&store);
	let wasm_ptr = WasmPtr::<u8>::new(result_offset);

	if let Ok(slice) = wasm_ptr.slice(&memory_view, bytes.len() as u32) {
		if let Err(e) = slice.write_slice(bytes) {
			tracing::error!("Failed to write to WASM memory: {:?}", e);
			return 0;
		}
	} else {
		tracing::error!("Failed to get WASM memory slice");
		return 0;
	}

	result_offset
}

fn write_error_to_memory(memory: &Memory, store: &mut wasmer::StoreMut, error: &str) -> u32 {
	let error_json = serde_json::json!({ "error": error });
	write_json_to_memory(memory, store, &error_json)
}

// === Job-Specific Host Functions ===

/// Report job progress
pub fn host_job_report_progress(
	mut env: FunctionEnvMut<PluginEnv>,
	job_id_ptr: WasmPtr<u8>,
	progress: f32,
	message_ptr: WasmPtr<u8>,
	message_len: u32,
) {
	let (plugin_env, mut store) = env.data_and_store_mut();
	let memory = &plugin_env.memory;
	let memory_view = memory.view(&store);

	let job_id = match read_uuid_from_wasm(&memory_view, job_id_ptr) {
		Ok(id) => id,
		Err(e) => {
			tracing::error!("Failed to read job ID: {}", e);
			return;
		}
	};

	let message = match read_string_from_wasm(&memory_view, message_ptr, message_len) {
		Ok(msg) => msg,
		Err(e) => {
			tracing::error!("Failed to read message: {}", e);
			return;
		}
	};

	tracing::info!(
		job_id = %job_id,
		progress = %progress,
		extension = %plugin_env.extension_id,
		"{}",
		message
	);

	// TODO: Forward to actual JobContext once registry is implemented
}

/// Save job checkpoint
pub fn host_job_checkpoint(
	mut env: FunctionEnvMut<PluginEnv>,
	job_id_ptr: WasmPtr<u8>,
	_state_ptr: WasmPtr<u8>,
	_state_len: u32,
) -> i32 {
	let (plugin_env, mut store) = env.data_and_store_mut();
	let memory = &plugin_env.memory;
	let memory_view = memory.view(&store);

	let job_id = match read_uuid_from_wasm(&memory_view, job_id_ptr) {
		Ok(id) => id,
		Err(e) => {
			tracing::error!("Failed to read job ID: {}", e);
			return 1; // Error
		}
	};

	tracing::debug!(job_id = %job_id, extension = %plugin_env.extension_id, "Checkpoint saved");

	// TODO: Actually save state to database
	0 // Success
}

/// Check if job should be interrupted
pub fn host_job_check_interrupt(
	mut env: FunctionEnvMut<PluginEnv>,
	job_id_ptr: WasmPtr<u8>,
) -> i32 {
	let (plugin_env, mut store) = env.data_and_store_mut();
	let memory = &plugin_env.memory;
	let memory_view = memory.view(&store);

	let _job_id = match read_uuid_from_wasm(&memory_view, job_id_ptr) {
		Ok(id) => id,
		Err(e) => {
			tracing::error!("Failed to read job ID: {}", e);
			return 0; // Continue
		}
	};

	// TODO: Check actual interrupt status
	0 // Not interrupted
}

/// Add job warning
pub fn host_job_add_warning(
	mut env: FunctionEnvMut<PluginEnv>,
	job_id_ptr: WasmPtr<u8>,
	message_ptr: WasmPtr<u8>,
	message_len: u32,
) {
	let (plugin_env, mut store) = env.data_and_store_mut();
	let memory = &plugin_env.memory;
	let memory_view = memory.view(&store);

	let job_id = match read_uuid_from_wasm(&memory_view, job_id_ptr) {
		Ok(id) => id,
		Err(_) => return,
	};

	let message = match read_string_from_wasm(&memory_view, message_ptr, message_len) {
		Ok(msg) => msg,
		Err(_) => return,
	};

	tracing::warn!(job_id = %job_id, extension = %plugin_env.extension_id, "Job warning: {}", message);
}

/// Increment bytes processed
pub fn host_job_increment_bytes(
	mut env: FunctionEnvMut<PluginEnv>,
	_job_id_ptr: WasmPtr<u8>,
	bytes: u64,
) {
	let (plugin_env, _store) = env.data_and_store_mut();
	tracing::debug!(extension = %plugin_env.extension_id, "Processed {} bytes", bytes);
	// TODO: Update metrics
}

/// Increment items processed
pub fn host_job_increment_items(
	mut env: FunctionEnvMut<PluginEnv>,
	_job_id_ptr: WasmPtr<u8>,
	count: u64,
) {
	let (plugin_env, _store) = env.data_and_store_mut();
	tracing::debug!(extension = %plugin_env.extension_id, "Processed {} items", count);
	// TODO: Update metrics
}

// === Extension Registration Functions ===

/// Register a job type for an extension
///
/// Called from plugin_init() to register custom job types
///
/// # Arguments
/// - `job_name_ptr`, `job_name_len`: Job name (e.g., "email_scan")
/// - `export_fn_ptr`, `export_fn_len`: WASM export function (e.g., "execute_email_scan")
/// - `resumable`: Whether the job supports resumption (1 = yes, 0 = no)
///
/// # Returns
/// 0 on success, 1 on error
pub fn host_register_job(
	mut env: FunctionEnvMut<PluginEnv>,
	job_name_ptr: WasmPtr<u8>,
	job_name_len: u32,
	export_fn_ptr: WasmPtr<u8>,
	export_fn_len: u32,
	resumable: u32,
) -> i32 {
	let (plugin_env, mut store) = env.data_and_store_mut();
	let memory = &plugin_env.memory;
	let memory_view = memory.view(&store);

	// Read job name
	let job_name = match read_string_from_wasm(&memory_view, job_name_ptr, job_name_len) {
		Ok(name) => name,
		Err(e) => {
			tracing::error!("Failed to read job name: {}", e);
			return 1; // Error
		}
	};

	// Read export function name
	let export_fn = match read_string_from_wasm(&memory_view, export_fn_ptr, export_fn_len) {
		Ok(name) => name,
		Err(e) => {
			tracing::error!("Failed to read export function name: {}", e);
			return 1; // Error
		}
	};

	let is_resumable = resumable != 0;

	// Register the job synchronously (no async needed)
	let result = plugin_env.job_registry.register(
		plugin_env.extension_id.clone(),
		job_name,
		export_fn,
		is_resumable,
	);

	match result {
		Ok(()) => 0, // Success
		Err(e) => {
			tracing::error!("Failed to register job: {}", e);
			1 // Error
		}
	}
}

#[cfg(test)]
mod bridge_tests {
	//! PLUG-002 — the plugin API bridge routes a generic `spacedrive_call`
	//! (method + library_id + JSON payload) through the Wire operation registry
	//! and returns the operation's result. `dispatch_extension_call` is the
	//! routing core `host_spacedrive_call` runs after reading those values out
	//! of WASM linear memory, so exercising it against a real `Core` proves a
	//! plugin can reach VDFS functionality end-to-end.
	//!
	//! Operations are addressed by their Wire method, i.e. the `query:`/`action:`
	//! prefixed name (`register_core_query!` registers `core.status` under
	//! `query:core.status`) — the same string the daemon RPC and the SDK use.

	use super::dispatch_extension_call;
	use crate::infra::api::ApiDispatcher;
	use crate::Core;
	use std::sync::Arc;
	use tempfile::TempDir;

	async fn setup() -> (TempDir, Core, Arc<ApiDispatcher>) {
		let temp = TempDir::new().unwrap();
		let core = Core::new(temp.path().to_path_buf()).await.unwrap();
		let dispatcher = Arc::new(ApiDispatcher::new(core.context.clone()));
		(temp, core, dispatcher)
	}

	#[tokio::test]
	async fn plugin_can_call_core_query_through_bridge() {
		let (_temp, core, dispatcher) = setup().await;

		// A plugin invoking a VDFS core query by Wire method name + JSON payload.
		// CoreStatusQuery::Input = (), so the payload is JSON null.
		let result = dispatch_extension_call(
			core.context.clone(),
			dispatcher,
			"query:core.status",
			None,
			serde_json::json!(null),
		)
		.await
		.expect("core.status should route through the bridge and return a result");

		assert!(
			result.is_object(),
			"core.status should return a status object, got {result:?}"
		);
	}

	#[tokio::test]
	async fn plugin_can_pass_payload_fields_through_bridge() {
		let (_temp, core, dispatcher) = setup().await;

		// libraries.list takes a real input struct; this proves payload fields
		// survive the bridge and reach the operation.
		let result = dispatch_extension_call(
			core.context.clone(),
			dispatcher,
			"query:libraries.list",
			None,
			serde_json::json!({ "include_stats": false }),
		)
		.await
		.expect("libraries.list should route through the bridge and return a result");

		// A freshly-created core has no libraries yet, so this is an empty list.
		assert!(
			result.is_array(),
			"libraries.list should return an array, got {result:?}"
		);
	}

	#[tokio::test]
	async fn unknown_method_is_rejected() {
		let (_temp, core, dispatcher) = setup().await;

		let err = dispatch_extension_call(
			core.context.clone(),
			dispatcher,
			"does.not.exist",
			None,
			serde_json::json!({}),
		)
		.await
		.expect_err("an unknown method must be rejected, not silently ignored");

		assert!(err.contains("Unknown method"), "unexpected error: {err}");
	}

	#[tokio::test]
	async fn library_scoped_method_requires_library_id() {
		let (_temp, core, dispatcher) = setup().await;

		// files.path_diff is a library-scoped query. Calling it without a
		// library id must be rejected before the operation runs, so a plugin
		// can't reach into library data without naming the library.
		let err = dispatch_extension_call(
			core.context.clone(),
			dispatcher,
			"query:files.path_diff",
			None,
			serde_json::json!({}),
		)
		.await
		.expect_err("a library query without a library id must be rejected");

		assert!(
			err.contains("Library ID required"),
			"unexpected error: {err}"
		);
	}

	#[tokio::test]
	async fn extension_specific_ops_are_registered() {
		let (_temp, core, dispatcher) = setup().await;

		// PLUG-002 follow-up: ai.ocr / credentials.store / vdfs.write_sidecar are
		// library-scoped actions. Reaching them proves they are registered on the
		// Wire registry (and therefore reachable through the bridge): dispatch gets
		// past the registry lookup to the library-id guard, so the error is
		// "Library ID required" — NOT "Unknown method", which is what an
		// unregistered op returns. Action Wire methods carry the `.input` suffix
		// (see the `action_method!` macro), matching how a real caller addresses them.
		for method in [
			"action:ai.ocr.input",
			"action:credentials.store.input",
			"action:vdfs.write_sidecar.input",
		] {
			let err = dispatch_extension_call(
				core.context.clone(),
				dispatcher.clone(),
				method,
				None,
				serde_json::json!({}),
			)
			.await
			.expect_err("a library action without a library id must be rejected");

			assert!(
				err.contains("Library ID required"),
				"{method} should be registered and require a library id, got: {err}"
			);
			assert!(
				!err.contains("Unknown method"),
				"{method} is not registered on the Wire registry: {err}"
			);
		}
	}

	/// End-to-end proof that a guest's `spacedrive_call` reaches the Wire registry
	/// through `host_spacedrive_call` — the memory-marshalling path the
	/// `dispatch_extension_call` unit tests skip. A minimal wasm guest forwards its
	/// arguments to the host import; the host reads the method + payload out of the
	/// guest's linear memory, dispatches, and writes the JSON result back into that
	/// same memory. We drive the guest and read the result pointer back out.
	#[tokio::test(flavor = "multi_thread")]
	async fn plugin_calls_core_query_over_wasm_memory() {
		use super::super::{
			job_registry::ExtensionJobRegistry, permissions::ExtensionPermissions,
			types::ManifestPermissions,
		};
		use super::{host_spacedrive_call, PluginEnv};
		use std::sync::Arc;
		use wasmer::{
			imports, wat2wasm, Function, FunctionEnv, Instance, Memory, Module, Store, Value,
			WasmPtr,
		};

		let (_temp, core, dispatcher) = setup().await;
		let core_context = core.context.clone();

		// `host_spacedrive_call` uses `Handle::current().block_on(...)` internally,
		// which panics inside an async task but is valid on a `spawn_blocking` thread
		// that still holds the runtime handle — the same context the plugin manager
		// invokes plugins in.
		let result = tokio::task::spawn_blocking(move || {
			// Minimal guest: imports `spacedrive_call`, exports `memory` (2 pages so
			// the host's 64 KiB result offset is in-bounds) and a `call` trampoline
			// that forwards its args straight to the host import.
			const WAT: &str = r#"
				(module
				  (import "env" "spacedrive_call"
				    (func $spacedrive_call (param i32 i32 i32 i32 i32) (result i32)))
				  (memory (export "memory") 2)
				  (func (export "call") (param i32 i32 i32 i32 i32) (result i32)
				    local.get 0 local.get 1 local.get 2 local.get 3 local.get 4
				    call $spacedrive_call))
			"#;

			let mut store = Store::default();
			// Compile from an explicit wasm binary rather than relying on
			// `Module::new` auto-detecting WAT text, so the test doesn't depend on
			// Wasmer's optional WAT-parsing behaviour.
			let wasm = wat2wasm(WAT.as_bytes()).expect("WAT parses to wasm");
			let module = Module::new(&store, wasm).expect("minimal guest module compiles");

			let manifest_perms = ManifestPermissions {
				methods: vec!["query:".to_string(), "action:".to_string()],
				..Default::default()
			};
			let permissions =
				ExtensionPermissions::from_manifest("roundtrip-test".to_string(), &manifest_perms);

			let temp_memory =
				Memory::new(&mut store, wasmer::MemoryType::new(1, None, false)).unwrap();
			let env = FunctionEnv::new(
				&mut store,
				PluginEnv {
					extension_id: "roundtrip-test".to_string(),
					core_context,
					api_dispatcher: dispatcher,
					permissions,
					memory: temp_memory,
					job_registry: Arc::new(ExtensionJobRegistry::new()),
				},
			);

			let import_object = imports! {
				"env" => {
					"spacedrive_call" =>
						Function::new_typed_with_env(&mut store, &env, host_spacedrive_call),
				}
			};

			let instance = Instance::new(&mut store, &module, &import_object)
				.expect("guest instantiates with the spacedrive_call import bound");

			// Point the env at the instance's real memory (the manager does the same).
			let memory = instance.exports.get_memory("memory").unwrap().clone();
			env.as_mut(&mut store).memory = memory.clone();

			// Marshal the call into linear memory as a guest would: method then
			// payload at known offsets, library id pointer 0 (None).
			let method = b"query:core.status";
			let payload = b"null";
			let method_ptr = 1024u32;
			let payload_ptr = 2048u32;
			{
				let view = memory.view(&store);
				view.write(method_ptr as u64, method).unwrap();
				view.write(payload_ptr as u64, payload).unwrap();
			}

			let call = instance.exports.get_function("call").unwrap();
			let results = call
				.call(
					&mut store,
					&[
						Value::I32(method_ptr as i32),
						Value::I32(method.len() as i32),
						Value::I32(0), // library_id_ptr == 0 => None
						Value::I32(payload_ptr as i32),
						Value::I32(payload.len() as i32),
					],
				)
				.expect("guest -> host bridge call succeeds");

			let result_ptr = match results[0] {
				Value::I32(p) => p as u32,
				ref other => panic!("unexpected return value: {other:?}"),
			};
			assert_ne!(
				result_ptr, 0,
				"host returned NULL, indicating a bridge error"
			);

			// Read the JSON the host wrote back. It has no length prefix and lands
			// in zero-filled memory, so read a window and cut at the first NUL.
			let view = memory.view(&store);
			let raw = WasmPtr::<u8>::new(result_ptr)
				.slice(&view, 4096)
				.unwrap()
				.read_to_vec()
				.unwrap();
			let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
			let json_str = String::from_utf8(raw[..end].to_vec()).unwrap();
			serde_json::from_str::<serde_json::Value>(&json_str)
				.expect("host wrote valid JSON back into guest memory")
		})
		.await
		.expect("blocking wasm task panicked");

		assert!(
			result.is_object() && result.get("error").is_none(),
			"core.status must round-trip through wasm memory as a status object, got {result:?}"
		);
	}
}
