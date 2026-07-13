//! Extension-facing VDFS operations (PLUG-002).
//!
//! Exposes `vdfs.write_sidecar` so a sandboxed plugin can persist derived data
//! (OCR text, transcripts, embeddings, …) for a content identity as a sidecar,
//! through the generic `spacedrive_call` bridge. The action writes the decoded
//! bytes to the library's sidecar store and records the sidecar in the database
//! via the shared [`SidecarManager`](crate::service::sidecar_manager::SidecarManager),
//! so it participates in availability tracking and resource events like any
//! natively-produced sidecar.

use crate::{
	context::CoreContext,
	infra::action::{error::ActionError, LibraryAction},
	ops::sidecar::{SidecarFormat, SidecarKind, SidecarVariant},
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Component, Path};
use std::sync::Arc;
use uuid::Uuid;

fn default_variant() -> String {
	"default".to_string()
}

/// Whether `variant` is a single normal path segment (no separators, no `..`,
/// not empty, not absolute). Used to reject path-traversal attempts from
/// untrusted extension input before the variant reaches the sidecar path.
fn is_safe_variant(variant: &str) -> bool {
	if variant.is_empty() {
		return false;
	}
	let mut components = Path::new(variant).components();
	matches!(
		(components.next(), components.next()),
		(Some(Component::Normal(_)), None)
	)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WriteSidecarInput {
	/// Content identity the sidecar is attached to.
	pub content_uuid: Uuid,
	/// Sidecar kind, e.g. "ocr", "transcript", "embeddings".
	pub kind: String,
	/// Variant discriminator (defaults to "default").
	#[serde(default = "default_variant")]
	pub variant: String,
	/// Storage format, e.g. "json", "txt", "msgpack".
	pub format: String,
	/// Sidecar payload, base64-encoded (standard alphabet).
	pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WriteSidecarOutput {
	/// Bytes written.
	pub size: u64,
	/// blake3 checksum of the written bytes (hex).
	pub checksum: String,
	/// Path of the sidecar relative to the library's sidecar directory.
	pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteSidecarAction {
	input: WriteSidecarInput,
}

impl WriteSidecarAction {
	pub fn new(input: WriteSidecarInput) -> Self {
		Self { input }
	}
}

impl LibraryAction for WriteSidecarAction {
	type Input = WriteSidecarInput;
	type Output = WriteSidecarOutput;

	fn from_input(input: WriteSidecarInput) -> Result<Self, String> {
		Ok(Self::new(input))
	}

	async fn execute(
		self,
		library: Arc<crate::library::Library>,
		context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let WriteSidecarInput {
			content_uuid,
			kind,
			variant,
			format,
			data_base64,
		} = self.input;

		// `variant` is attacker-controlled: it comes straight from a sandboxed
		// WASM extension and is later formatted into the sidecar filename
		// (`{variant}.{ext}`) and pushed onto the on-disk path. Reject anything
		// that isn't a single normal path segment, otherwise separators or `..`
		// would let a malicious extension escape the sidecar directory (path
		// traversal). `kind`/`format` are enums parsed below and `content_uuid`
		// is a UUID, so `variant` is the only free-form path input.
		if !is_safe_variant(&variant) {
			return Err(ActionError::InvalidInput(format!(
				"invalid sidecar variant {variant:?}: must be a single path segment \
				 with no separators or parent-directory references"
			)));
		}

		let kind = SidecarKind::try_from(kind.as_str()).map_err(ActionError::InvalidInput)?;
		let format = SidecarFormat::try_from(format.as_str()).map_err(ActionError::InvalidInput)?;
		let variant = SidecarVariant::new(variant);

		let data = base64::engine::general_purpose::STANDARD
			.decode(data_base64.as_bytes())
			.map_err(|e| {
				ActionError::InvalidInput(format!("data_base64 is not valid base64: {}", e))
			})?;

		let manager = context.get_sidecar_manager().await.ok_or_else(|| {
			ActionError::Internal("Sidecar manager is not initialized".to_string())
		})?;

		let path = manager
			.compute_path(&library.id(), &content_uuid, &kind, &variant, &format)
			.await
			.map_err(|e| ActionError::Internal(format!("Failed to compute sidecar path: {}", e)))?;

		if let Some(parent) = path.absolute_path.parent() {
			tokio::fs::create_dir_all(parent).await.map_err(|e| {
				ActionError::Internal(format!("Failed to create sidecar directory: {}", e))
			})?;
		}
		tokio::fs::write(&path.absolute_path, &data)
			.await
			.map_err(|e| ActionError::Internal(format!("Failed to write sidecar: {}", e)))?;

		let size = data.len() as u64;
		let checksum = blake3::hash(&data).to_hex().to_string();

		manager
			.record_sidecar(
				&library,
				&content_uuid,
				&kind,
				&variant,
				&format,
				size,
				Some(checksum.clone()),
			)
			.await
			.map_err(|e| ActionError::Internal(format!("Failed to record sidecar: {}", e)))?;

		Ok(WriteSidecarOutput {
			size,
			checksum,
			relative_path: path.relative_path.to_string_lossy().to_string(),
		})
	}

	fn action_kind(&self) -> &'static str {
		"vdfs.write_sidecar"
	}
}

crate::register_library_action!(WriteSidecarAction, "vdfs.write_sidecar");

#[cfg(test)]
mod tests {
	use super::is_safe_variant;

	#[test]
	fn accepts_ordinary_variants() {
		for v in ["default", "grid@2x", "v1.2", "thumbnail_512", "a..b", "..."] {
			assert!(is_safe_variant(v), "{v:?} should be accepted");
		}
	}

	#[test]
	fn rejects_path_traversal_and_separators() {
		for v in [
			"",
			"..",
			".",
			"../secret",
			"../../etc/passwd",
			"foo/bar",
			"/abs",
			"/etc/passwd",
			"a/..",
			"./x",
		] {
			assert!(!is_safe_variant(v), "{v:?} should be rejected");
		}
	}
}
