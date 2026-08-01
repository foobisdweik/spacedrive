use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DeleteSidecarInput {
	pub content_uuid: Uuid,
	pub kind: String,
	pub variant: String,
}
