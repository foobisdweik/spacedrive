use crate::{
	context::CoreContext,
	domain::addressing::SdPath,
	infra::query::{LibraryQuery, QueryError, QueryResult},
	ops::indexing::{
		database_storage::EntryMetadata, state::EntryKind, IndexScope, IndexerJob, IndexerJobConfig,
	},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
	sync::Arc,
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PathDiffInput {
	pub source: SdPath,
	pub target: SdPath,
	#[serde(default)]
	pub strategy: DiffStrategy,
	#[serde(default)]
	pub use_index_rules: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DiffStrategy {
	#[default]
	Heuristic,
	Content,
	Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiffEntry {
	pub relative_path: PathBuf,
	pub sd_path: SdPath,
	pub uuid: Option<Uuid>,
	pub size: u64,
	pub modified_at: Option<DateTime<Utc>>,
	pub content_id: Option<String>,
	pub kind: DiffEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DiffEntryKind {
	File,
	Directory,
	Symlink,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PathDiffResult {
	pub only_in_source: Vec<DiffEntry>,
	pub only_in_target: Vec<DiffEntry>,
	pub modified: Vec<DiffEntry>,
	pub matched_count: usize,
	pub copy_size: u64,
	pub total_scanned: usize,
}

pub struct PathDiffQuery {
	input: PathDiffInput,
}

impl LibraryQuery for PathDiffQuery {
	type Input = PathDiffInput;
	type Output = PathDiffResult;

	fn from_input(input: Self::Input) -> QueryResult<Self> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		context: Arc<CoreContext>,
		session: crate::infra::api::SessionContext,
	) -> QueryResult<Self::Output> {
		let library_id = session
			.current_library_id
			.ok_or_else(|| QueryError::Internal("No library in session".to_string()))?;
		let library = context
			.libraries()
			.await
			.get_library(library_id)
			.await
			.ok_or_else(|| QueryError::Internal("Library not found".to_string()))?;

		let source = resolve_local_path(&self.input.source, &context).await?;
		let target = resolve_local_path(&self.input.target, &context).await?;

		ensure_indexed(&library, &source, self.input.use_index_rules).await?;
		ensure_indexed(&library, &target, self.input.use_index_rules).await?;

		let cache = library.core_context().ephemeral_cache();
		let source_index = cache.get_for_path(&source).ok_or_else(|| {
			QueryError::Internal(format!("Path not indexed: {}", source.display()))
		})?;
		let target_index = cache.get_for_path(&target).ok_or_else(|| {
			QueryError::Internal(format!("Path not indexed: {}", target.display()))
		})?;

		let source_map = {
			let index = source_index.read().await;
			build_path_map(&source, &index.entries(), |path| index.get_entry_uuid(path))
		};
		let target_map = {
			let index = target_index.read().await;
			build_path_map(&target, &index.entries(), |path| index.get_entry_uuid(path))
		};

		Ok(diff_path_maps(
			&source_map,
			&target_map,
			self.input.strategy,
		))
	}
}

async fn resolve_local_path(path: &SdPath, context: &Arc<CoreContext>) -> QueryResult<PathBuf> {
	let resolved = path
		.resolve(context)
		.await
		.map_err(|e| QueryError::Internal(format!("Failed to resolve path: {}", e)))?;

	resolved
		.as_local_path()
		.map(Path::to_path_buf)
		.ok_or_else(|| QueryError::Internal("Path diff currently requires local paths".to_string()))
}

async fn ensure_indexed(
	library: &crate::library::Library,
	path: &Path,
	use_index_rules: bool,
) -> QueryResult<()> {
	let cache = library.core_context().ephemeral_cache();

	if cache.is_indexed(path) {
		return Ok(());
	}

	let config = if use_index_rules {
		IndexerJobConfig::ephemeral_browse(
			SdPath::local(path.to_path_buf()),
			IndexScope::Recursive,
			false,
		)
	} else {
		IndexerJobConfig::complete_scan(SdPath::local(path.to_path_buf()), IndexScope::Recursive)
	};

	let handle = library
		.jobs()
		.dispatch(IndexerJob::new(config))
		.await
		.map_err(|e| QueryError::Internal(format!("Failed to dispatch indexer: {}", e)))?;

	handle
		.wait()
		.await
		.map_err(|e| QueryError::Internal(format!("Indexer failed: {}", e)))?;

	Ok(())
}

fn build_path_map<F>(
	root: &Path,
	entries: &HashMap<PathBuf, EntryMetadata>,
	uuid_for_path: F,
) -> HashMap<PathBuf, DiffEntry>
where
	F: Fn(&PathBuf) -> Option<Uuid>,
{
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

		map.insert(
			relative_path.to_path_buf(),
			build_diff_entry(
				relative_path,
				absolute_path,
				metadata,
				uuid_for_path(absolute_path),
			),
		);
	}

	map
}

fn build_diff_entry(
	relative_path: &Path,
	absolute_path: &Path,
	metadata: &EntryMetadata,
	uuid: Option<Uuid>,
) -> DiffEntry {
	DiffEntry {
		relative_path: relative_path.to_path_buf(),
		sd_path: SdPath::local(absolute_path.to_path_buf()),
		uuid,
		size: metadata.size,
		modified_at: metadata.modified.map(DateTime::<Utc>::from),
		content_id: None,
		kind: DiffEntryKind::from(metadata.kind),
	}
}

fn diff_path_maps(
	source_map: &HashMap<PathBuf, DiffEntry>,
	target_map: &HashMap<PathBuf, DiffEntry>,
	strategy: DiffStrategy,
) -> PathDiffResult {
	match strategy {
		DiffStrategy::Heuristic | DiffStrategy::Hybrid | DiffStrategy::Content => {
			diff_heuristic(source_map, target_map)
		}
	}
}

fn diff_heuristic(
	source_map: &HashMap<PathBuf, DiffEntry>,
	target_map: &HashMap<PathBuf, DiffEntry>,
) -> PathDiffResult {
	let mut result = PathDiffResult {
		total_scanned: source_map.len() + target_map.len(),
		..Default::default()
	};

	for (relative_path, source_entry) in source_map {
		match target_map.get(relative_path) {
			None => result.only_in_source.push(source_entry.clone()),
			Some(target_entry) if entries_match_heuristically(source_entry, target_entry) => {
				result.matched_count += 1;
			}
			Some(_) => result.modified.push(source_entry.clone()),
		}
	}

	for (relative_path, target_entry) in target_map {
		if !source_map.contains_key(relative_path) {
			result.only_in_target.push(target_entry.clone());
		}
	}

	sort_entries(&mut result.only_in_source);
	sort_entries(&mut result.only_in_target);
	sort_entries(&mut result.modified);

	result.copy_size = result
		.only_in_source
		.iter()
		.chain(result.modified.iter())
		.map(|entry| entry.size)
		.sum();

	result
}

fn entries_match_heuristically(source: &DiffEntry, target: &DiffEntry) -> bool {
	source.kind == target.kind
		&& source.size == target.size
		&& source.modified_at == target.modified_at
}

fn sort_entries(entries: &mut [DiffEntry]) {
	entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
}

impl From<EntryKind> for DiffEntryKind {
	fn from(kind: EntryKind) -> Self {
		match kind {
			EntryKind::File => Self::File,
			EntryKind::Directory => Self::Directory,
			EntryKind::Symlink => Self::Symlink,
		}
	}
}

crate::register_library_query!(PathDiffQuery, "files.path_diff");

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(relative_path: &str, size: u64, modified_at: i64) -> DiffEntry {
		DiffEntry {
			relative_path: PathBuf::from(relative_path),
			sd_path: SdPath::local(PathBuf::from("/source").join(relative_path)),
			uuid: None,
			size,
			modified_at: DateTime::from_timestamp(modified_at, 0),
			content_id: None,
			kind: DiffEntryKind::File,
		}
	}

	#[test]
	fn heuristic_diff_finds_missing_modified_and_extra_entries() {
		let source_map = HashMap::from([
			(PathBuf::from("same.txt"), entry("same.txt", 10, 1)),
			(PathBuf::from("missing.txt"), entry("missing.txt", 20, 2)),
			(PathBuf::from("changed.txt"), entry("changed.txt", 30, 3)),
		]);
		let target_map = HashMap::from([
			(PathBuf::from("same.txt"), entry("same.txt", 10, 1)),
			(PathBuf::from("changed.txt"), entry("changed.txt", 31, 3)),
			(PathBuf::from("extra.txt"), entry("extra.txt", 40, 4)),
		]);

		let result = diff_heuristic(&source_map, &target_map);

		assert_eq!(result.matched_count, 1);
		assert_eq!(result.total_scanned, 6);
		assert_eq!(result.copy_size, 50);
		assert_eq!(
			result.only_in_source[0].relative_path,
			PathBuf::from("missing.txt")
		);
		assert_eq!(
			result.modified[0].relative_path,
			PathBuf::from("changed.txt")
		);
		assert_eq!(
			result.only_in_target[0].relative_path,
			PathBuf::from("extra.txt")
		);
	}

	#[test]
	fn heuristic_diff_sorts_entries_by_relative_path() {
		let source_map = HashMap::from([
			(PathBuf::from("z.txt"), entry("z.txt", 1, 1)),
			(PathBuf::from("a.txt"), entry("a.txt", 1, 1)),
		]);
		let target_map = HashMap::new();

		let result = diff_heuristic(&source_map, &target_map);

		assert_eq!(
			result.only_in_source[0].relative_path,
			PathBuf::from("a.txt")
		);
		assert_eq!(
			result.only_in_source[1].relative_path,
			PathBuf::from("z.txt")
		);
	}
}
