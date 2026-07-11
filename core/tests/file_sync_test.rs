//! File Sync Integration Test
//!
//! This test validates the file sync feature end-to-end:
//! 1. Create a library with test data
//! 2. Set up source and target directories with files
//! 3. Index both directories as entries
//! 4. Create a sync conduit between them
//! 5. Trigger a sync operation
//! 6. Verify files were synchronized correctly
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test -p sd-core --test file_sync_test -- --test-threads=1
//! ```

use sd_core::{
	infra::db::entities::{directory_paths, entry, sync_conduit},
	Core,
};
use sea_orm::{ActiveModelTrait, Set};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::fs;
use uuid::Uuid;

/// Helper to create test files with content
async fn create_test_file(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).await?;
	}
	fs::write(path, content).await?;
	Ok(())
}

/// Test setup with a core and library
struct FileSyncTestSetup {
	_temp_dir: TempDir,
	core: Core,
	library: Arc<sd_core::library::Library>,
	data_root: PathBuf,
}

impl FileSyncTestSetup {
	/// Create a new test setup
	async fn new() -> anyhow::Result<Self> {
		let _ = tracing_subscriber::fmt()
			.with_env_filter("sd_core=debug,file_sync_test=debug")
			.with_test_writer()
			.try_init();

		let temp_dir = TempDir::new()?;

		let config = sd_core::config::AppConfig {
			version: 3,
			data_dir: temp_dir.path().to_path_buf(),
			log_level: "info".to_string(),
			telemetry_enabled: false,
			preferences: sd_core::config::Preferences::default(),
			job_logging: sd_core::config::JobLoggingConfig::default(),
			services: sd_core::config::ServiceConfig {
				networking_enabled: false,
				volume_monitoring_enabled: false,
				fs_watcher_enabled: false,
				statistics_listener_enabled: false,
			},
			logging: sd_core::config::LoggingConfig::default(),
			proxy_pairing: sd_core::config::ProxyPairingConfig::default(),
			spacebot: sd_core::config::SpacebotConfig::default(),
		};
		config.save()?;

		let core = Core::new(temp_dir.path().to_path_buf())
			.await
			.map_err(|e| anyhow::anyhow!("{}", e))?;

		let library = core
			.libraries
			.create_library("File Sync Test Library", None, core.context.clone())
			.await?;

		// Initialize file sync service
		library.init_file_sync_service()?;

		let data_root = temp_dir.path().join("sync_data");
		fs::create_dir_all(&data_root).await?;

		Ok(Self {
			_temp_dir: temp_dir,
			core,
			library,
			data_root,
		})
	}

	/// Create a test entry in the database
	async fn create_entry(
		&self,
		name: &str,
		kind: i32,
		parent_id: Option<i32>,
		size: i64,
	) -> anyhow::Result<entry::Model> {
		let now = chrono::Utc::now();

		let entry = entry::ActiveModel {
			uuid: Set(Some(Uuid::new_v4())),
			name: Set(name.to_string()),
			kind: Set(kind),
			extension: Set(None),
			metadata_id: Set(None),
			content_id: Set(None),
			size: Set(size),
			aggregate_size: Set(size),
			child_count: Set(0),
			file_count: Set(if kind == 0 { 1 } else { 0 }),
			created_at: Set(now),
			modified_at: Set(now),
			accessed_at: Set(None),
			indexed_at: Set(Some(now)),
			permissions: Set(None),
			inode: Set(None),
			parent_id: Set(parent_id),
			..Default::default()
		};

		Ok(entry.insert(self.library.db().conn()).await?)
	}

	/// Create a real directory on disk plus its entry and directory_paths row
	async fn create_dir_entry(&self, name: &str) -> anyhow::Result<(entry::Model, PathBuf)> {
		let dir_path = self.data_root.join(name);
		fs::create_dir_all(&dir_path).await?;
		let dir_path = fs::canonicalize(&dir_path).await?;

		let entry = self.create_entry(name, 1, None, 0).await?;

		directory_paths::ActiveModel {
			entry_id: Set(entry.id),
			path: Set(dir_path.to_string_lossy().to_string()),
		}
		.insert(self.library.db().conn())
		.await?;

		Ok((entry, dir_path))
	}

	/// Create a real file on disk plus its entry under a parent directory
	async fn create_file_entry(
		&self,
		parent: &entry::Model,
		parent_path: &Path,
		name: &str,
		content: &str,
	) -> anyhow::Result<entry::Model> {
		let file_path = parent_path.join(name);
		create_test_file(&file_path, content).await?;
		self.create_entry(name, 0, Some(parent.id), content.len() as i64)
			.await
	}

	/// Wait until the sync for a conduit finishes (or time out)
	async fn wait_for_sync(&self, conduit_id: i32) -> anyhow::Result<()> {
		let file_sync = self
			.library
			.file_sync_service()
			.ok_or_else(|| anyhow::anyhow!("File sync service not initialized"))?;

		for _ in 0..600 {
			if !file_sync.is_syncing(conduit_id).await {
				return Ok(());
			}
			tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
		}

		Err(anyhow::anyhow!("Sync did not complete within timeout"))
	}
}

#[tokio::test]
async fn test_file_sync_service_initialization() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	// Verify file sync service was initialized
	assert!(setup.library.file_sync_service().is_some());

	println!("✓ File sync service initialized successfully");
}

#[tokio::test]
async fn test_conduit_creation() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	// Create source and target directory entries
	let source_dir = setup.create_entry("source", 1, None, 0).await.unwrap();
	let target_dir = setup.create_entry("target", 1, None, 0).await.unwrap();

	// Get file sync service
	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	// Create a sync conduit
	let conduit = conduit_manager
		.create_conduit(
			source_dir.id,
			target_dir.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	// Verify conduit was created
	assert_eq!(conduit.source_entry_id, source_dir.id);
	assert_eq!(conduit.target_entry_id, target_dir.id);
	assert_eq!(conduit.sync_mode, "mirror");
	assert!(conduit.enabled);
	assert_eq!(conduit.sync_generation, 0);
	assert_eq!(conduit.total_syncs, 0);

	println!("✓ Sync conduit created successfully");
	println!(
		"  Source: {} (ID: {})",
		source_dir.name, conduit.source_entry_id
	);
	println!(
		"  Target: {} (ID: {})",
		target_dir.name, conduit.target_entry_id
	);
	println!("  Mode: {}", conduit.sync_mode);
}

#[tokio::test]
async fn test_conduit_list() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	// Create multiple directory entries
	let dir1 = setup.create_entry("dir1", 1, None, 0).await.unwrap();
	let dir2 = setup.create_entry("dir2", 1, None, 0).await.unwrap();
	let dir3 = setup.create_entry("dir3", 1, None, 0).await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	// Create multiple conduits
	conduit_manager
		.create_conduit(
			dir1.id,
			dir2.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	conduit_manager
		.create_conduit(
			dir2.id,
			dir3.id,
			sync_conduit::SyncMode::Bidirectional,
			"interval:5m".to_string(),
		)
		.await
		.unwrap();

	// List all conduits
	let all_conduits = conduit_manager.list_all().await.unwrap();
	assert_eq!(all_conduits.len(), 2);

	// List enabled conduits
	let enabled_conduits = conduit_manager.list_enabled().await.unwrap();
	assert_eq!(enabled_conduits.len(), 2);

	println!("✓ Created and listed {} conduits", all_conduits.len());
	for conduit in &all_conduits {
		println!(
			"  Conduit {}: {} -> {} ({})",
			conduit.id, conduit.source_entry_id, conduit.target_entry_id, conduit.sync_mode
		);
	}
}

#[tokio::test]
async fn test_conduit_enable_disable() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	let source = setup.create_entry("source", 1, None, 0).await.unwrap();
	let target = setup.create_entry("target", 1, None, 0).await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	let conduit = conduit_manager
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	assert!(conduit.enabled);

	// Disable the conduit
	conduit_manager
		.set_enabled(conduit.id, false)
		.await
		.unwrap();

	let updated = conduit_manager.get_conduit(conduit.id).await.unwrap();
	assert!(!updated.enabled);

	// Re-enable
	conduit_manager.set_enabled(conduit.id, true).await.unwrap();

	let updated = conduit_manager.get_conduit(conduit.id).await.unwrap();
	assert!(updated.enabled);

	println!("✓ Conduit enable/disable working correctly");
}

#[tokio::test]
async fn test_mirror_sync_empty_to_empty() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	// Create empty source and target directories
	let (source, _source_path) = setup.create_dir_entry("source_empty").await.unwrap();
	let (target, _target_path) = setup.create_dir_entry("target_empty").await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	// Create sync conduit
	let conduit = conduit_manager
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	// Trigger sync
	let handle = file_sync.sync_now(conduit.id).await.unwrap();

	println!("✓ Mirror sync completed for empty directories");
	println!("  Generation: {}", handle.generation);
	println!(
		"  Copy job: {:?}",
		handle.source_to_target.copy_job_id.is_some()
	);
	println!(
		"  Delete job: {:?}",
		handle.source_to_target.delete_job_id.is_some()
	);

	// Verify no jobs were created (nothing to sync)
	assert!(handle.source_to_target.copy_job_id.is_none());
	assert!(handle.source_to_target.delete_job_id.is_none());
}

#[tokio::test]
async fn test_mirror_sync_with_files() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	// Create source directory with files
	let (source, source_path) = setup.create_dir_entry("source_dir").await.unwrap();
	let _file1 = setup
		.create_file_entry(&source, &source_path, "file1.txt", "file one contents")
		.await
		.unwrap();
	let _file2 = setup
		.create_file_entry(&source, &source_path, "file2.txt", "file two contents!")
		.await
		.unwrap();

	// Create empty target directory
	let (target, target_path) = setup.create_dir_entry("target_dir").await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	// Create sync conduit
	let conduit = conduit_manager
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	// Trigger sync
	let handle = file_sync.sync_now(conduit.id).await.unwrap();

	println!("✓ Mirror sync started with files");
	println!("  Source files: 2 (file1.txt, file2.txt)");
	println!("  Target files: 0");
	println!("  Generation: {}", handle.generation);
	println!(
		"  Copy job created: {}",
		handle.source_to_target.copy_job_id.is_some()
	);

	// Verify copy job was created
	assert!(handle.source_to_target.copy_job_id.is_some());

	// Wait for the monitor to finish copies, deletes and verification
	setup.wait_for_sync(conduit.id).await.unwrap();

	// Verify files were copied to the target
	assert!(target_path.join("file1.txt").exists());
	assert!(target_path.join("file2.txt").exists());

	// Verify conduit state was updated
	let updated_conduit = conduit_manager.get_conduit(conduit.id).await.unwrap();
	assert_eq!(updated_conduit.sync_generation, 1);
	assert_eq!(updated_conduit.total_syncs, 1);

	println!("  Updated generation: {}", updated_conduit.sync_generation);
	println!("  Total syncs: {}", updated_conduit.total_syncs);
}

#[tokio::test]
async fn test_sync_resolver_calculates_operations() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	// Create source with files
	let (source, source_path) = setup.create_dir_entry("source").await.unwrap();
	let _file1 = setup
		.create_file_entry(&source, &source_path, "common.txt", "common contents")
		.await
		.unwrap();
	let _file2 = setup
		.create_file_entry(&source, &source_path, "source_only.txt", "source only")
		.await
		.unwrap();

	// Create target with one extraneous file
	let (target, target_path) = setup.create_dir_entry("target").await.unwrap();
	let _file3 = setup
		.create_file_entry(&target, &target_path, "target_only.txt", "target only")
		.await
		.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	let conduit = conduit_manager
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	// Calculate operations and dispatch jobs
	let handle = file_sync.sync_now(conduit.id).await.unwrap();

	println!("✓ Sync resolver calculated operations");
	println!("  Files in source: 2");
	println!("  Files in target: 1");
	println!("  Expected: Copy 2 files, Delete 1 file");
	println!(
		"  Copy job created: {}",
		handle.source_to_target.copy_job_id.is_some()
	);

	// Copy job dispatched immediately; deletes are deferred until copies finish
	assert!(handle.source_to_target.copy_job_id.is_some());

	setup.wait_for_sync(conduit.id).await.unwrap();

	// Source files mirrored to target
	assert!(target_path.join("common.txt").exists());
	assert!(target_path.join("source_only.txt").exists());

	// Extraneous target file deleted
	assert!(!target_path.join("target_only.txt").exists());

	println!("  Mirror sync copied source files and deleted extraneous target file");
}

#[tokio::test]
async fn test_cannot_sync_disabled_conduit() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	let source = setup.create_entry("source", 1, None, 0).await.unwrap();
	let target = setup.create_entry("target", 1, None, 0).await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	let conduit = conduit_manager
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	// Disable the conduit
	conduit_manager
		.set_enabled(conduit.id, false)
		.await
		.unwrap();

	// Try to sync - should fail
	let result = file_sync.sync_now(conduit.id).await;
	assert!(result.is_err());
	assert!(result.unwrap_err().to_string().contains("disabled"));

	println!("✓ Cannot sync disabled conduit (as expected)");
}

#[tokio::test]
async fn test_cannot_sync_same_conduit_twice() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	let (source, source_path) = setup.create_dir_entry("source").await.unwrap();
	for i in 0..5 {
		setup
			.create_file_entry(
				&source,
				&source_path,
				&format!("file{}.txt", i),
				&format!("contents of file {}", i),
			)
			.await
			.unwrap();
	}
	let (target, _target_path) = setup.create_dir_entry("target").await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	let conduit = conduit_manager
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	// Start first sync
	let _handle1 = file_sync.sync_now(conduit.id).await.unwrap();

	// Try to start second sync immediately - should fail
	let result = file_sync.sync_now(conduit.id).await;
	assert!(result.is_err());
	assert!(result.unwrap_err().to_string().contains("in progress"));

	setup.wait_for_sync(conduit.id).await.unwrap();

	println!("✓ Cannot start concurrent syncs for same conduit (as expected)");
}

#[tokio::test]
async fn test_generation_tracking() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	let (source, _source_path) = setup.create_dir_entry("source").await.unwrap();
	let (target, _target_path) = setup.create_dir_entry("target").await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit_manager = file_sync.conduit_manager();

	let conduit = conduit_manager
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	assert_eq!(conduit.sync_generation, 0);

	// First sync
	let _handle1 = file_sync.sync_now(conduit.id).await.unwrap();
	tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

	let conduit = conduit_manager.get_conduit(conduit.id).await.unwrap();
	assert_eq!(conduit.sync_generation, 1);

	// Second sync
	let _handle2 = file_sync.sync_now(conduit.id).await.unwrap();
	tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

	let conduit = conduit_manager.get_conduit(conduit.id).await.unwrap();
	assert_eq!(conduit.sync_generation, 2);

	println!("✓ Generation tracking working correctly");
	println!("  Generation increments on each sync: 0 -> 1 -> 2");
}
