//! Integration tests for rules-free ephemeral complete scan mode (INDEX-011)
//!
//! File sync and diff operations need to see every file on disk. A "complete
//! scan" bypasses all indexer rules (node_modules, .git, gitignore, dev dirs)
//! and is additive: it fills gaps left by an earlier filtered scan without
//! duplicating entries or churning their UUIDs.

mod helpers;

use helpers::*;
use sd_core::{
	domain::addressing::SdPath,
	ops::indexing::{IndexScope, IndexerJob, IndexerJobConfig},
};
use std::path::Path;

async fn run_ephemeral_scan(
	harness: &IndexingHarness,
	config: IndexerJobConfig,
	root: &Path,
) -> anyhow::Result<()> {
	let cache = harness.core.context.ephemeral_cache();
	let index = cache.create_for_indexing(root.to_path_buf(), IndexScope::Recursive);
	let mut job = IndexerJob::new(config);
	job.set_ephemeral_index(index);
	let handle = harness.library.jobs().dispatch(job).await?;
	handle.wait().await?;
	Ok(())
}

#[tokio::test]
async fn test_complete_scan_includes_rule_filtered_files() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("complete_scan")
		.disable_watcher()
		.build()
		.await?;

	// Build a directory with content that default rules exclude.
	let root = harness.temp_path().join("complete_scan_root");
	tokio::fs::create_dir_all(root.join("node_modules/pkg")).await?;
	tokio::fs::create_dir_all(root.join(".git")).await?;
	tokio::fs::write(root.join("normal.txt"), "normal").await?;
	tokio::fs::write(root.join("node_modules/pkg/index.js"), "js").await?;
	tokio::fs::write(root.join(".git/config"), "[core]").await?;

	let node_modules_file = root.join("node_modules/pkg/index.js");
	let git_file = root.join(".git/config");
	let normal_file = root.join("normal.txt");

	// 1. Filtered ephemeral scan (default rules) skips dev dirs and .git.
	run_ephemeral_scan(
		&harness,
		IndexerJobConfig::ephemeral_browse(
			SdPath::local(root.clone()),
			IndexScope::Recursive,
			false,
		),
		&root,
	)
	.await?;

	let cache = harness.core.context.ephemeral_cache();
	let filtered_uuid = {
		let index = cache.get_global_index();
		let index = index.read().await;
		assert!(
			index.has_entry(&normal_file),
			"filtered scan must index regular files"
		);
		assert!(
			!index.has_entry(&node_modules_file),
			"filtered scan must exclude node_modules (no_dev_dirs)"
		);
		assert!(
			!index.has_entry(&git_file),
			"filtered scan must exclude .git contents (no_git)"
		);
		index
			.get_entry_uuid(&normal_file)
			.expect("filtered scan assigns a uuid to regular files")
	};

	// 2. Complete scan fills the gaps without touching existing entries.
	run_ephemeral_scan(
		&harness,
		IndexerJobConfig::complete_scan(SdPath::local(root.clone()), IndexScope::Recursive),
		&root,
	)
	.await?;

	{
		let index = cache.get_global_index();
		let index = index.read().await;
		assert!(
			index.has_entry(&node_modules_file),
			"complete scan must include node_modules contents"
		);
		assert!(
			index.has_entry(&git_file),
			"complete scan must include .git contents"
		);
		assert_eq!(
			index.get_entry_uuid(&normal_file),
			Some(filtered_uuid),
			"complete scan must preserve UUIDs assigned by the filtered scan"
		);

		// Additive, not duplicating: exactly one entry per path.
		let normal_entries = index
			.entries()
			.iter()
			.filter(|(path, _)| **path == normal_file)
			.count();
		assert_eq!(
			normal_entries, 1,
			"complete scan must not duplicate entries"
		);
	}

	harness.shutdown().await
}
