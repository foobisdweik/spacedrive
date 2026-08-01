use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SetFavoriteInput {
	pub entry_uuid: Uuid,
	pub favorite: bool,
}
