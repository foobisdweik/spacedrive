//! Location operations

use crate::{
	domain::{resource::Identifiable, Location},
	infra::event::{Event, EventBus, ResourceMetadata},
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

pub mod add;
pub mod enable_indexing;
pub mod export;
pub mod import;
pub mod list;
pub mod remove;
pub mod rescan;
pub mod suggested;
pub mod trigger_job;
pub mod update;
pub mod validate;

pub use add::*;
pub use enable_indexing::*;
pub use export::*;
pub use import::*;
pub use list::*;
pub use remove::*;
pub use rescan::*;
pub use suggested::*;
pub use trigger_job::*;
pub use update::*;
pub use validate::*;

// Register validation query
crate::register_library_query!(
	validate::ValidateLocationPathQuery,
	"locations.validate_path"
);

pub(crate) async fn emit_location_changed_batch(
	db: &DatabaseConnection,
	events: &EventBus,
	library_id: Uuid,
	ids: &[Uuid],
) -> crate::common::errors::Result<()> {
	if ids.is_empty() {
		return Ok(());
	}

	let mut locations = Location::from_ids(db, ids).await?;
	for location in &mut locations {
		location.library_id = library_id;
		if let crate::domain::addressing::SdPath::Physical { path, .. } = &location.sd_path {
			location.is_available = tokio::fs::try_exists(path).await.unwrap_or(false);
		}
	}

	if locations.is_empty() {
		return Ok(());
	}

	events.emit(Event::ResourceChangedBatch {
		resource_type: Location::resource_type().to_string(),
		resources: serde_json::to_value(locations).map_err(|error| {
			crate::common::errors::CoreError::Other(anyhow::anyhow!(
				"Failed to serialize location resources: {error}"
			))
		})?,
		metadata: Some(ResourceMetadata {
			no_merge_fields: Location::no_merge_fields()
				.iter()
				.map(|field| field.to_string())
				.collect(),
			alternate_ids: vec![],
			affected_paths: vec![],
		}),
	});

	Ok(())
}
