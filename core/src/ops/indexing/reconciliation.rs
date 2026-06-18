use crate::infra::db::entities::{directory_paths, entry, entry_closure};
use crate::ops::indexing::path_resolver::PathResolver;
use anyhow::Result;
use async_trait::async_trait;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, QueryTrait};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[async_trait]
pub trait PersistentUuidLookup: Send + Sync {
	async fn lookup_uuid(&self, path: &Path) -> Option<Uuid>;
}

pub async fn extract_persistent_uuids_for_path(
	db: &DatabaseConnection,
	root_path: &Path,
) -> Result<HashMap<PathBuf, Uuid>> {
	let root_str = root_path.to_string_lossy().to_string();

	// Find the directory_paths entry for root
	let root_dir = directory_paths::Entity::find()
		.filter(directory_paths::Column::Path.eq(&root_str))
		.one(db)
		.await?;

	let Some(root_dir) = root_dir else {
		return Ok(HashMap::new()); // Path not in persistent index
	};

	// Keep the closure-table lookup inside SQLite to avoid parameter limits on
	// large directories.
	let descendants = entry::Entity::find()
		.filter(
			entry::Column::Id.in_subquery(
				entry_closure::Entity::find()
					.select_only()
					.column(entry_closure::Column::DescendantId)
					.filter(entry_closure::Column::AncestorId.eq(root_dir.entry_id))
					.into_query(),
			),
		)
		.filter(entry::Column::Uuid.is_not_null())
		.all(db)
		.await?;

	// Resolve full paths using directory_paths cache + filename
	let mut result = HashMap::with_capacity(descendants.len());
	for entry in descendants {
		if let Ok(full_path) = PathResolver::get_full_path(db, entry.id).await {
			if let Some(uuid) = entry.uuid {
				result.insert(full_path, uuid);
			}
		}
	}

	Ok(result)
}
