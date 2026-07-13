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
use std::sync::Arc;
use uuid::Uuid;

fn default_variant() -> String {
	"default".to_string()
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
