use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use super::{DeleteSidecarInput, DeleteSidecarOutput};
use crate::{
	context::CoreContext,
	domain::ResourceManager,
	infra::{
		action::{error::ActionError, LibraryAction},
		db::entities::{content_identity, entry, sidecar},
		sync::ChangeType,
	},
	library::Library,
	ops::sidecar::{SidecarKind, SidecarVariant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSidecarAction {
	input: DeleteSidecarInput,
}

impl LibraryAction for DeleteSidecarAction {
	type Input = DeleteSidecarInput;
	type Output = DeleteSidecarOutput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		SidecarKind::try_from(input.kind.as_str())?;
		if input.variant.trim().is_empty() {
			return Err("Sidecar variant cannot be empty".to_string());
		}

		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let kind =
			SidecarKind::try_from(self.input.kind.as_str()).map_err(ActionError::InvalidInput)?;
		let variant = SidecarVariant::from(self.input.variant.clone());
		let db = library.db().conn();

		let sidecars = sidecar::Entity::find()
			.filter(sidecar::Column::ContentUuid.eq(self.input.content_uuid))
			.filter(sidecar::Column::Kind.eq(kind.as_str()))
			.filter(sidecar::Column::Variant.eq(variant.as_str()))
			.all(db)
			.await
			.map_err(ActionError::SeaOrm)?;

		if sidecars.is_empty() {
			return Err(ActionError::InvalidInput(
				"The selected sidecar no longer exists".to_string(),
			));
		}

		let manager = context
			.get_sidecar_manager()
			.await
			.ok_or_else(|| ActionError::Internal("Sidecar manager is unavailable".to_string()))?;
		manager
			.remove_sidecar(&library, &self.input.content_uuid, &kind, &variant)
			.await
			.map_err(|error| ActionError::Internal(format!("Failed to delete sidecar: {error}")))?;

		for sidecar in &sidecars {
			library
				.sync_model(sidecar, ChangeType::Delete)
				.await
				.map_err(|error| {
					ActionError::Internal(format!("Failed to sync sidecar deletion: {error}"))
				})?;
		}

		let entry_uuids = if let Some(content) = content_identity::Entity::find()
			.filter(content_identity::Column::Uuid.eq(self.input.content_uuid))
			.one(db)
			.await
			.map_err(ActionError::SeaOrm)?
		{
			entry::Entity::find()
				.filter(entry::Column::ContentId.eq(content.id))
				.all(db)
				.await
				.map_err(ActionError::SeaOrm)?
				.into_iter()
				.filter_map(|entry| entry.uuid)
				.collect()
		} else {
			Vec::new()
		};

		if !entry_uuids.is_empty() {
			ResourceManager::new(Arc::new(db.clone()), context.events.clone())
				.emit_resource_events("file", entry_uuids)
				.await
				.map_err(|error| {
					ActionError::Internal(format!("Failed to emit sidecar deletion: {error}"))
				})?;
		}

		Ok(DeleteSidecarOutput {
			content_uuid: self.input.content_uuid,
			kind: self.input.kind,
			variant: self.input.variant,
			deleted_count: sidecars.len(),
		})
	}

	fn action_kind(&self) -> &'static str {
		"sidecar.delete"
	}
}

crate::register_library_action!(DeleteSidecarAction, "sidecar.delete");
