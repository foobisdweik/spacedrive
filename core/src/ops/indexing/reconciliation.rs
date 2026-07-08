use crate::infra::db::entities::{directory_paths, entry, entry_closure, location};
use crate::ops::indexing::path_resolver::PathResolver;
use anyhow::Result;
use async_trait::async_trait;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, QueryTrait};
use std::collections::HashMap;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use uuid::Uuid;

#[async_trait]
pub trait PersistentUuidLookup: Send + Sync {
	async fn lookup_uuid(&self, path: &Path) -> Option<Uuid>;
}

/// A single ephemeral→persistent UUID reconciliation.
#[derive(Debug, Clone)]
pub struct ReconciledUuid {
	pub path: PathBuf,
	/// The ephemeral UUID that was replaced, if the entry had one.
	pub previous: Option<Uuid>,
	/// The persistent UUID now assigned.
	pub uuid: Uuid,
}

/// Extract persistent entry UUIDs for every entry that overlaps `root_path`.
///
/// Overlap is bidirectional:
/// - `root_path` lies inside (or at the root of) a persistent tree — resolved
///   via an exact `directory_paths` match, which covers all descendants.
/// - `root_path` is an ancestor of one or more persistent location roots
///   (e.g. an ephemeral scan of `/Users/james` with a location at
///   `/Users/james/Documents`) — resolved by extracting each such root.
pub async fn extract_persistent_uuids_for_path(
	db: &DatabaseConnection,
	root_path: &Path,
) -> Result<HashMap<PathBuf, Uuid>> {
	let root_str = root_path.to_string_lossy().to_string();

	// Direction 1: the scanned path is itself a persistent directory. Its
	// subtree covers any nested location roots, so no further lookup needed.
	let exact_root = directory_paths::Entity::find()
		.filter(directory_paths::Column::Path.eq(&root_str))
		.one(db)
		.await?;

	let extraction_roots: Vec<i32> = if let Some(dir) = exact_root {
		vec![dir.entry_id]
	} else {
		// Direction 2: find persistent location roots strictly under the
		// scanned path. Locations are few, so filter their cached paths here.
		let location_roots: Vec<i32> = location::Entity::find()
			.filter(location::Column::EntryId.is_not_null())
			.all(db)
			.await?
			.into_iter()
			.filter_map(|l| l.entry_id)
			.collect();

		if location_roots.is_empty() {
			return Ok(HashMap::new());
		}

		let prefix = if root_str.ends_with(MAIN_SEPARATOR) {
			root_str.clone()
		} else {
			format!("{root_str}{MAIN_SEPARATOR}")
		};

		directory_paths::Entity::find()
			.filter(directory_paths::Column::EntryId.is_in(location_roots))
			.all(db)
			.await?
			.into_iter()
			.filter(|dp| dp.path.starts_with(&prefix))
			.map(|dp| dp.entry_id)
			.collect()
	};

	let mut result = HashMap::new();
	for root_entry_id in extraction_roots {
		collect_subtree_uuids(db, root_entry_id, &mut result).await?;
	}

	Ok(result)
}

/// Collect `path → uuid` for every entry in the subtree rooted at
/// `root_entry_id` that has a UUID.
async fn collect_subtree_uuids(
	db: &DatabaseConnection,
	root_entry_id: i32,
	out: &mut HashMap<PathBuf, Uuid>,
) -> Result<()> {
	// Keep the closure-table lookup inside SQLite to avoid parameter limits on
	// large directories.
	let descendants = entry::Entity::find()
		.filter(
			entry::Column::Id.in_subquery(
				entry_closure::Entity::find()
					.select_only()
					.column(entry_closure::Column::DescendantId)
					.filter(entry_closure::Column::AncestorId.eq(root_entry_id))
					.into_query(),
			),
		)
		.filter(entry::Column::Uuid.is_not_null())
		.all(db)
		.await?;

	// Resolve full paths using directory_paths cache + filename
	out.reserve(descendants.len());
	for entry in descendants {
		if let Ok(full_path) = PathResolver::get_full_path(db, entry.id).await {
			if let Some(uuid) = entry.uuid {
				out.insert(full_path, uuid);
			}
		}
	}

	Ok(())
}
