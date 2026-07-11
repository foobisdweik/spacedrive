use crate::{
	domain::addressing::SdPath,
	infra::db::entities::{directory_paths, entry, sync_conduit, sync_generation},
	library::Library,
	ops::indexing::{
		database_storage::EntryMetadata, state::EntryKind, IndexScope, IndexerJob, IndexerJobConfig,
	},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sea_orm::{prelude::*, DatabaseConnection, QueryOrder, QuerySelect};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Calculates sync operations from index queries
pub struct SyncResolver {
	db: Arc<DatabaseConnection>,
	library: Arc<Library>,
}

/// Entry with its materialized path relative to sync root
#[derive(Debug, Clone)]
pub struct EntryWithPath {
	pub entry: entry::Model,
	pub relative_path: PathBuf,
	pub full_path: PathBuf,
}

impl EntryWithPath {
	/// Convert to SdPath for job operations
	pub fn to_sdpath(&self, device_slug: String) -> SdPath {
		SdPath::physical(device_slug, self.full_path.clone())
	}
}

/// Operations for a single sync direction
#[derive(Debug, Default, Clone)]
pub struct DirectionalOps {
	pub destination_root: Option<PathBuf>,
	pub to_copy: Vec<EntryWithPath>,
	pub to_delete: Vec<EntryWithPath>,
}

/// Complete sync operations (supports bidirectional)
#[derive(Debug, Default)]
pub struct SyncOperations {
	/// Source → target operations
	pub source_to_target: DirectionalOps,

	/// Target → source operations (only for bidirectional mode)
	pub target_to_source: Option<DirectionalOps>,

	/// Conflicts that need resolution
	pub conflicts: Vec<SyncConflict>,
}

impl SyncOperations {
	/// True when no copy/delete operations or conflicts remain.
	pub fn is_converged(&self) -> bool {
		self.source_to_target.to_copy.is_empty()
			&& self.source_to_target.to_delete.is_empty()
			&& self
				.target_to_source
				.as_ref()
				.map(|ops| ops.to_copy.is_empty() && ops.to_delete.is_empty())
				.unwrap_or(true)
			&& self.conflicts.is_empty()
	}
}

#[derive(Debug, Clone)]
pub struct SyncConflict {
	pub relative_path: PathBuf,
	pub source_entry: EntryWithPath,
	pub target_entry: EntryWithPath,
	pub conflict_type: ConflictType,
}

#[derive(Debug, Clone, Copy)]
pub enum ConflictType {
	BothModified,
	DeletedVsModified,
	TypeMismatch,
}

impl SyncResolver {
	pub fn new(db: Arc<DatabaseConnection>, library: Arc<Library>) -> Self {
		Self { db, library }
	}

	/// Calculate sync operations for a conduit
	pub async fn calculate_operations(
		&self,
		conduit: &sync_conduit::Model,
	) -> Result<SyncOperations> {
		let source_root_path = self
			.path_for_directory_entry(conduit.source_entry_id)
			.await?;
		let target_root_path = self
			.path_for_directory_entry(conduit.target_entry_id)
			.await?;

		let mode = sync_conduit::SyncMode::from_str(&conduit.sync_mode)
			.ok_or_else(|| anyhow::anyhow!("Invalid sync mode"))?;

		if !conduit.use_index_rules {
			self.ensure_complete_scan(&source_root_path).await?;
			self.ensure_complete_scan(&target_root_path).await?;

			let source_map = self.build_ephemeral_path_map(&source_root_path).await?;
			let target_map = self.build_ephemeral_path_map(&target_root_path).await?;

			return match mode {
				sync_conduit::SyncMode::Mirror => {
					Ok(self.resolve_mirror(&source_map, &target_map, target_root_path))
				}
				sync_conduit::SyncMode::Bidirectional => {
					self.resolve_bidirectional(
						&source_map,
						&target_map,
						conduit,
						source_root_path,
						target_root_path,
					)
					.await
				}
				sync_conduit::SyncMode::Selective => {
					Ok(self.resolve_mirror(&source_map, &target_map, target_root_path))
				}
			};
		}

		// Get source and target root entries
		let source_root = entry::Entity::find_by_id(conduit.source_entry_id)
			.one(&*self.db)
			.await?
			.ok_or_else(|| anyhow::anyhow!("Source entry not found"))?;

		let target_root = entry::Entity::find_by_id(conduit.target_entry_id)
			.one(&*self.db)
			.await?
			.ok_or_else(|| anyhow::anyhow!("Target entry not found"))?;

		// Load all entries under each root
		let source_entries = self
			.get_entries_recursive(conduit.source_entry_id, &source_root, &source_root_path)
			.await?;
		let target_entries = self
			.get_entries_recursive(conduit.target_entry_id, &target_root, &target_root_path)
			.await?;

		// Build path maps
		let source_map = self.build_path_map(&source_entries);
		let target_map = self.build_path_map(&target_entries);

		match mode {
			sync_conduit::SyncMode::Mirror => {
				Ok(self.resolve_mirror(&source_map, &target_map, target_root_path))
			}
			sync_conduit::SyncMode::Bidirectional => {
				self.resolve_bidirectional(
					&source_map,
					&target_map,
					conduit,
					source_root_path,
					target_root_path,
				)
				.await
			}
			sync_conduit::SyncMode::Selective => {
				Ok(self.resolve_mirror(&source_map, &target_map, target_root_path))
			}
		}
	}

	/// Re-resolve a conduit against a fresh complete filesystem scan.
	///
	/// Used by the Trust Watcher verification flow: after the sync jobs finish
	/// and the index has settled, both endpoints are re-scanned and the sync is
	/// re-resolved. A converged sync produces no remaining operations.
	pub async fn verify_conduit(&self, conduit: &sync_conduit::Model) -> Result<SyncOperations> {
		let source_root_path = self
			.path_for_directory_entry(conduit.source_entry_id)
			.await?;
		let target_root_path = self
			.path_for_directory_entry(conduit.target_entry_id)
			.await?;

		self.ensure_complete_scan(&source_root_path).await?;
		self.ensure_complete_scan(&target_root_path).await?;

		let source_map = self.build_ephemeral_path_map(&source_root_path).await?;
		let target_map = self.build_ephemeral_path_map(&target_root_path).await?;

		let mode = sync_conduit::SyncMode::from_str(&conduit.sync_mode)
			.ok_or_else(|| anyhow::anyhow!("Invalid sync mode"))?;

		match mode {
			sync_conduit::SyncMode::Bidirectional => {
				self.resolve_bidirectional(
					&source_map,
					&target_map,
					conduit,
					source_root_path,
					target_root_path,
				)
				.await
			}
			_ => Ok(self.resolve_mirror(&source_map, &target_map, target_root_path)),
		}
	}

	async fn ensure_complete_scan(&self, path: &Path) -> Result<()> {
		let cache = self.library.core_context().ephemeral_cache();
		let index = cache.create_for_indexing(path.to_path_buf(), IndexScope::Recursive);
		cache.clear_for_reindex(path).await;

		let config = IndexerJobConfig::complete_scan(
			SdPath::local(path.to_path_buf()),
			IndexScope::Recursive,
		);
		let mut job = IndexerJob::new(config);
		job.set_ephemeral_index(index);

		let handle = self.library.jobs().dispatch(job).await?;
		handle.wait().await?;

		Ok(())
	}

	async fn path_for_directory_entry(&self, entry_id: i32) -> Result<PathBuf> {
		let directory_path = directory_paths::Entity::find_by_id(entry_id)
			.one(&*self.db)
			.await?
			.ok_or_else(|| anyhow::anyhow!("Directory path not found for entry {}", entry_id))?;

		let path = PathBuf::from(directory_path.path);
		Ok(tokio::fs::canonicalize(&path).await.unwrap_or(path))
	}

	async fn build_ephemeral_path_map(
		&self,
		root: &Path,
	) -> Result<HashMap<PathBuf, EntryWithPath>> {
		let cache = self.library.core_context().ephemeral_cache();
		let index = cache
			.get_for_path(root)
			.ok_or_else(|| anyhow::anyhow!("Path not indexed: {}", root.display()))?;
		let index = index.read().await;
		let entries = index.entries();
		let mut map = HashMap::new();

		for (absolute_path, metadata) in entries {
			if absolute_path == root {
				continue;
			}

			let Ok(relative_path) = absolute_path.strip_prefix(root) else {
				continue;
			};

			if relative_path.as_os_str().is_empty() {
				continue;
			}

			let relative_path = relative_path.to_path_buf();
			map.insert(
				relative_path.clone(),
				EntryWithPath {
					entry: entry_model_from_metadata(
						&absolute_path,
						&metadata,
						index.get_entry_uuid(&absolute_path),
					),
					relative_path,
					full_path: absolute_path,
				},
			);
		}

		Ok(map)
	}

	/// Get all entries under a directory recursively
	/// This is a simplified implementation - in a real implementation,
	/// we'd need to reconstruct full paths by walking parent relationships
	async fn get_entries_recursive(
		&self,
		root_id: i32,
		root_entry: &entry::Model,
		root_path: &Path,
	) -> Result<Vec<EntryWithPath>> {
		let mut results = Vec::new();

		// Simple recursive query - find all entries with this root as ancestor
		// This is a simplified approach. In production, we'd need proper path reconstruction
		let entries = self.find_children_recursive(root_id).await?;

		// For MVP, we'll use a simple relative path construction
		// In production, this should walk parent links to build full paths
		for entry in entries {
			let relative_path = PathBuf::from(&entry.name);
			let full_path = root_path.join(&relative_path);

			results.push(EntryWithPath {
				entry,
				relative_path,
				full_path,
			});
		}

		Ok(results)
	}

	/// Find all children of an entry recursively using parent_id relationship
	async fn find_children_recursive(&self, parent_id: i32) -> Result<Vec<entry::Model>> {
		let mut all_children = Vec::new();
		let mut to_process = vec![parent_id];

		while let Some(current_parent) = to_process.pop() {
			let children = entry::Entity::find()
				.filter(entry::Column::ParentId.eq(current_parent))
				.all(&*self.db)
				.await?;

			for child in children {
				to_process.push(child.id);
				all_children.push(child);
			}
		}

		Ok(all_children)
	}

	/// Build map of relative path -> entry with path
	fn build_path_map(&self, entries: &[EntryWithPath]) -> HashMap<PathBuf, EntryWithPath> {
		entries
			.iter()
			.map(|e| (e.relative_path.clone(), e.clone()))
			.collect()
	}

	/// Resolve mirror mode: source -> target (one-way)
	fn resolve_mirror(
		&self,
		source_map: &HashMap<PathBuf, EntryWithPath>,
		target_map: &HashMap<PathBuf, EntryWithPath>,
		target_root: PathBuf,
	) -> SyncOperations {
		let mut operations = SyncOperations::default();
		operations.source_to_target.destination_root = Some(target_root);

		// Files in source but not target, or files that differ -> copy
		for (path, source_entry_with_path) in source_map {
			if let Some(target_entry_with_path) = target_map.get(path) {
				// File exists in both - check if content differs
				if self
					.content_differs(&source_entry_with_path.entry, &target_entry_with_path.entry)
				{
					operations
						.source_to_target
						.to_copy
						.push(source_entry_with_path.clone());
				}
			} else {
				// File only in source - copy it
				operations
					.source_to_target
					.to_copy
					.push(source_entry_with_path.clone());
			}
		}

		// Files in target but not source -> delete
		for (path, target_entry_with_path) in target_map {
			if !source_map.contains_key(path) {
				operations
					.source_to_target
					.to_delete
					.push(target_entry_with_path.clone());
			}
		}

		operations
	}

	/// Resolve bidirectional mode with conflict detection
	async fn resolve_bidirectional(
		&self,
		source_map: &HashMap<PathBuf, EntryWithPath>,
		target_map: &HashMap<PathBuf, EntryWithPath>,
		conduit: &sync_conduit::Model,
		source_root: PathBuf,
		target_root: PathBuf,
	) -> Result<SyncOperations> {
		let mut operations = SyncOperations::default();
		operations.source_to_target.destination_root = Some(target_root);
		operations.target_to_source = Some(DirectionalOps {
			destination_root: Some(source_root),
			..Default::default()
		});

		// Get last sync generation for change detection
		let last_gen = self.get_last_completed_generation(conduit.id).await?;

		// Detect changes since last sync
		let source_changes = self.detect_changes(source_map, last_gen.as_ref());
		let target_changes = self.detect_changes(target_map, last_gen.as_ref());

		// Check each file in both locations
		let all_paths: HashSet<_> = source_map
			.keys()
			.chain(target_map.keys())
			.cloned()
			.collect();

		for path in all_paths {
			let in_source = source_map.get(&path);
			let in_target = target_map.get(&path);

			match (in_source, in_target) {
				(Some(source_entry_with_path), Some(target_entry_with_path)) => {
					// File in both locations
					let source_changed = source_changes.contains(&path);
					let target_changed = target_changes.contains(&path);

					if source_changed && target_changed {
						// Conflict: both modified
						operations.conflicts.push(SyncConflict {
							relative_path: path.clone(),
							source_entry: source_entry_with_path.clone(),
							target_entry: target_entry_with_path.clone(),
							conflict_type: ConflictType::BothModified,
						});
					} else if source_changed {
						// Source changed, target unchanged -> copy to target
						operations
							.source_to_target
							.to_copy
							.push(source_entry_with_path.clone());
					} else if target_changed {
						// Target changed, source unchanged -> copy to source
						if let Some(ref mut target_to_source) = operations.target_to_source {
							target_to_source
								.to_copy
								.push(target_entry_with_path.clone());
						}
					}
				}
				(Some(source_entry_with_path), None) => {
					// Only in source -> copy to target
					operations
						.source_to_target
						.to_copy
						.push(source_entry_with_path.clone());
				}
				(None, Some(target_entry_with_path)) => {
					// Only in target -> copy to source
					if let Some(ref mut target_to_source) = operations.target_to_source {
						target_to_source
							.to_copy
							.push(target_entry_with_path.clone());
					}
				}
				(None, None) => unreachable!(),
			}
		}

		Ok(operations)
	}

	fn content_differs(&self, entry1: &entry::Model, entry2: &entry::Model) -> bool {
		// Compare content identity
		match (entry1.content_id, entry2.content_id) {
			(Some(c1), Some(c2)) => c1 != c2,
			// If either doesn't have content_id, compare by size and modified time
			_ => entry1.size != entry2.size || entry1.modified_at != entry2.modified_at,
		}
	}

	fn detect_changes(
		&self,
		entries: &HashMap<PathBuf, EntryWithPath>,
		last_gen: Option<&sync_generation::Model>,
	) -> HashSet<PathBuf> {
		let mut changed = HashSet::new();

		if let Some(gen) = last_gen {
			let last_sync_time = gen.completed_at.unwrap_or(gen.started_at);
			for (path, entry_with_path) in entries {
				// Check if entry was modified after last sync
				if let Some(indexed_at) = entry_with_path.entry.indexed_at {
					if indexed_at > last_sync_time {
						changed.insert(path.clone());
					}
				}
			}
		} else {
			// No previous sync - all files are "changes"
			changed.extend(entries.keys().cloned());
		}

		changed
	}

	async fn get_last_completed_generation(
		&self,
		conduit_id: i32,
	) -> Result<Option<sync_generation::Model>> {
		Ok(sync_generation::Entity::find()
			.filter(sync_generation::Column::ConduitId.eq(conduit_id))
			.filter(sync_generation::Column::CompletedAt.is_not_null())
			.order_by_desc(sync_generation::Column::Generation)
			.one(&*self.db)
			.await?)
	}
}

fn entry_model_from_metadata(
	path: &Path,
	metadata: &EntryMetadata,
	uuid: Option<uuid::Uuid>,
) -> entry::Model {
	let modified_at = metadata
		.modified
		.map(DateTime::<Utc>::from)
		.unwrap_or_else(Utc::now);
	let created_at = metadata
		.created
		.map(DateTime::<Utc>::from)
		.unwrap_or(modified_at);

	entry::Model {
		id: 0,
		uuid,
		name: path
			.file_name()
			.map(|name| name.to_string_lossy().to_string())
			.unwrap_or_default(),
		kind: match metadata.kind {
			EntryKind::File => 0,
			EntryKind::Directory => 1,
			EntryKind::Symlink => 2,
		},
		extension: path
			.extension()
			.map(|extension| extension.to_string_lossy().to_string()),
		metadata_id: None,
		content_id: None,
		size: i64::try_from(metadata.size).unwrap_or(i64::MAX),
		aggregate_size: 0,
		child_count: 0,
		file_count: 0,
		created_at,
		modified_at,
		accessed_at: metadata.accessed.map(DateTime::<Utc>::from),
		indexed_at: Some(modified_at),
		permissions: metadata
			.permissions
			.map(|permissions| permissions.to_string()),
		inode: metadata.inode.and_then(|inode| i64::try_from(inode).ok()),
		parent_id: None,
		volume_id: None,
	}
}
