//! File Sync Verification & History Tests
//!
//! Covers the library-sync-completeness gate, the Trust Watcher verification
//! flow, and generation history queries.
//!
//! ```bash
//! cargo test -p sd-core --test file_sync_verification_test -- --test-threads=1
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

struct FileSyncTestSetup {
	_temp_dir: TempDir,
	_core: Core,
	library: Arc<sd_core::library::Library>,
	data_root: PathBuf,
}

impl FileSyncTestSetup {
	async fn new() -> anyhow::Result<Self> {
		let _ = tracing_subscriber::fmt()
			.with_env_filter("sd_core=info")
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
			.create_library("File Sync Verification Library", None, core.context.clone())
			.await?;

		library.init_file_sync_service()?;

		let data_root = temp_dir.path().join("sync_data");
		fs::create_dir_all(&data_root).await?;

		Ok(Self {
			_temp_dir: temp_dir,
			_core: core,
			library,
			data_root,
		})
	}

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

	async fn create_file_entry(
		&self,
		parent: &entry::Model,
		parent_path: &Path,
		name: &str,
		content: &str,
	) -> anyhow::Result<entry::Model> {
		let file_path = parent_path.join(name);
		fs::write(&file_path, content).await?;
		self.create_entry(name, 0, Some(parent.id), content.len() as i64)
			.await
	}

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
async fn test_cannot_sync_when_library_sync_incomplete() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	let (source, _) = setup.create_dir_entry("source").await.unwrap();
	let (target, _) = setup.create_dir_entry("target").await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit = file_sync
		.conduit_manager()
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	// Simulate a library that is still backfilling metadata
	file_sync.set_library_sync_ready_override(Some(false)).await;

	let result = file_sync.sync_now(conduit.id).await;
	assert!(result.is_err());
	assert!(result.unwrap_err().to_string().contains("Library sync"));

	// Once library sync completes, the same conduit can sync
	file_sync.set_library_sync_ready_override(Some(true)).await;
	let handle = file_sync.sync_now(conduit.id).await;
	assert!(handle.is_ok());

	println!("✓ Library sync gate blocks stale-index syncs");
}

#[tokio::test]
async fn test_library_sync_complete_without_sync_service() {
	let setup = FileSyncTestSetup::new().await.unwrap();
	let file_sync = setup.library.file_sync_service().unwrap();

	// Standalone library (no distributed sync) is always complete
	assert!(file_sync.is_library_sync_complete().await);

	println!("✓ Standalone library treated as sync-complete");
}

#[tokio::test]
async fn test_verification_marks_generation_verified() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	let (source, source_path) = setup.create_dir_entry("source").await.unwrap();
	setup
		.create_file_entry(&source, &source_path, "a.txt", "alpha")
		.await
		.unwrap();
	setup
		.create_file_entry(&source, &source_path, "b.txt", "bravo")
		.await
		.unwrap();
	let (target, target_path) = setup.create_dir_entry("target").await.unwrap();
	setup
		.create_file_entry(&target, &target_path, "stale.txt", "stale")
		.await
		.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit = file_sync
		.conduit_manager()
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	let handle = file_sync.sync_now(conduit.id).await.unwrap();
	setup.wait_for_sync(conduit.id).await.unwrap();

	// Filesystem converged
	assert!(target_path.join("a.txt").exists());
	assert!(target_path.join("b.txt").exists());
	assert!(!target_path.join("stale.txt").exists());

	// Trust Watcher verification re-resolved the conduit and found convergence
	let generation = file_sync
		.history()
		.get_generation(conduit.id, handle.generation)
		.await
		.unwrap()
		.expect("generation record should exist");

	assert!(generation.completed_at.is_some());
	assert_eq!(generation.verification_status, "verified");
	assert!(generation.verified_at.is_some());
	assert_eq!(generation.files_copied, 2);
	assert_eq!(generation.files_deleted, 1);

	println!("✓ Generation verified via Trust Watcher flow");
}

#[tokio::test]
async fn test_generation_history_queries() {
	let setup = FileSyncTestSetup::new().await.unwrap();

	let (source, source_path) = setup.create_dir_entry("source").await.unwrap();
	setup
		.create_file_entry(&source, &source_path, "one.txt", "one")
		.await
		.unwrap();
	let (target, _target_path) = setup.create_dir_entry("target").await.unwrap();

	let file_sync = setup.library.file_sync_service().unwrap();
	let conduit = file_sync
		.conduit_manager()
		.create_conduit(
			source.id,
			target.id,
			sync_conduit::SyncMode::Mirror,
			"manual".to_string(),
		)
		.await
		.unwrap();

	let handle = file_sync.sync_now(conduit.id).await.unwrap();
	setup.wait_for_sync(conduit.id).await.unwrap();

	let history = file_sync.history();

	let generations = history.list_generations(conduit.id, 10).await.unwrap();
	assert_eq!(generations.len(), 1);
	assert_eq!(generations[0].generation, handle.generation);

	let latest = history.latest_generation(conduit.id).await.unwrap();
	assert_eq!(latest.map(|g| g.generation), Some(handle.generation));

	let completed = history.last_completed_generation(conduit.id).await.unwrap();
	assert_eq!(completed.map(|g| g.generation), Some(handle.generation));

	let verified = history.last_verified_generation(conduit.id).await.unwrap();
	assert_eq!(verified.map(|g| g.generation), Some(handle.generation));

	let stats = history.stats(conduit.id).await.unwrap();
	assert_eq!(stats.total_generations, 1);
	assert_eq!(stats.completed_generations, 1);
	assert_eq!(stats.verified_generations, 1);
	assert_eq!(stats.failed_generations, 0);
	assert_eq!(stats.files_copied, 1);
	assert_eq!(stats.files_deleted, 0);

	println!("✓ Generation history queries return expected records");
}
