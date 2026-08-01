use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use super::{SetFavoriteInput, SetFavoriteOutput};
use crate::{
	context::CoreContext,
	domain::ResourceManager,
	infra::{
		action::{error::ActionError, LibraryAction},
		db::entities::entry,
		sync::ChangeType,
	},
	library::Library,
	ops::metadata::UserMetadataManager,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetFavoriteAction {
	input: SetFavoriteInput,
}

impl LibraryAction for SetFavoriteAction {
	type Input = SetFavoriteInput;
	type Output = SetFavoriteOutput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let db = library.db().conn();
		let entry_exists = entry::Entity::find()
			.filter(entry::Column::Uuid.eq(self.input.entry_uuid))
			.one(db)
			.await
			.map_err(ActionError::SeaOrm)?
			.is_some();

		if !entry_exists {
			return Err(ActionError::InvalidInput(
				"This file must be indexed before it can be favorited".to_string(),
			));
		}

		let manager = UserMetadataManager::new(Arc::new(db.clone()));
		let (metadata, created, updated_metadata) = manager
			.set_favorite(self.input.entry_uuid, self.input.favorite)
			.await
			.map_err(|error| ActionError::Internal(error.to_string()))?;

		for updated in updated_metadata {
			if let Err(error) = library.sync_model(&updated, ChangeType::Update).await {
				tracing::warn!(
					error = %error,
					metadata_uuid = %updated.uuid,
					"favorite metadata persisted but its sync update could not be queued"
				);
			}
		}

		if let Err(error) = library
			.sync_model(
				&metadata,
				if created {
					ChangeType::Insert
				} else {
					ChangeType::Update
				},
			)
			.await
		{
			tracing::warn!(
				error = %error,
				metadata_uuid = %metadata.uuid,
				"favorite metadata persisted but its sync change could not be queued"
			);
		}

		let resource_manager = ResourceManager::new(Arc::new(db.clone()), context.events.clone());
		if let Err(error) = resource_manager
			.emit_resource_events("file", vec![self.input.entry_uuid])
			.await
		{
			tracing::warn!(
				error = %error,
				entry_uuid = %self.input.entry_uuid,
				"favorite metadata persisted but its resource update could not be emitted"
			);
		}

		Ok(SetFavoriteOutput {
			entry_uuid: self.input.entry_uuid,
			favorite: self.input.favorite,
		})
	}

	fn action_kind(&self) -> &'static str {
		"metadata.set_favorite"
	}
}

crate::register_library_action!(SetFavoriteAction, "metadata.set_favorite");
