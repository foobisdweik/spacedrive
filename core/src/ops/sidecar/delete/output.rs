use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DeleteSidecarOutput {
	pub content_uuid: Uuid,
	pub kind: String,
	pub variant: String,
	pub deleted_count: usize,
}
