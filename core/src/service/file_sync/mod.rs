use crate::{
	context::CoreContext,
	domain::addressing::{SdPath, SdPathBatch},
	infra::{
		db::entities::{sync_conduit, sync_generation},
		event::Event,
		job::types::JobId,
	},
	library::Library,
	ops::files::{
		copy::{job::CopyOptions, job::FileCopyJob},
		delete::{job::DeleteJob, job::DeleteMode},
	},
};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub mod conduit;
pub mod conflict;
pub mod history;
pub mod resolver;

use conduit::ConduitManager;
use history::SyncHistory;
use resolver::{DirectionalOps, SyncResolver};

/// File sync orchestration service
pub struct FileSyncService {
	library: Arc<Library>,
	conduit_manager: Arc<ConduitManager>,
	resolver: Arc<SyncResolver>,
	history: Arc<SyncHistory>,

	/// Active sync operations (conduit_id -> sync operation)
	active_syncs: Arc<RwLock<HashMap<i32, SyncOperation>>>,

	/// Test override for library-sync readiness. `None` defers to the real
	/// distributed sync state; `Some(ready)` forces the gate open or closed.
	library_sync_ready_override: Arc<RwLock<Option<bool>>>,
}

/// Tracks jobs for a single sync direction
#[derive(Debug, Clone)]
pub struct JobBatch {
	pub copy_job_id: Option<JobId>,
	pub delete_job_id: Option<JobId>,
}

/// Active sync operation tracking
#[derive(Debug)]
struct SyncOperation {
	conduit_id: i32,
	generation: i64,
	generation_id: i32,
	started_at: chrono::DateTime<chrono::Utc>,

	/// Jobs for source → target direction
	source_to_target: JobBatch,
	source_to_target_delete_ops: DirectionalOps,

	/// Jobs for target → source direction (only for bidirectional mode)
	target_to_source: Option<JobBatch>,
	target_to_source_delete_ops: Option<DirectionalOps>,
}

impl FileSyncService {
	pub fn new(library: Arc<Library>) -> Self {
		let db = Arc::new(library.db().conn().clone());
		let conduit_manager = Arc::new(ConduitManager::new(db.clone()));
		let resolver = Arc::new(SyncResolver::new(db.clone(), library.clone()));
		let history = Arc::new(SyncHistory::new(db));

		Self {
			library,
			conduit_manager,
			resolver,
			history,
			active_syncs: Arc::new(RwLock::new(HashMap::new())),
			library_sync_ready_override: Arc::new(RwLock::new(None)),
		}
	}

	pub async fn cancel_sync(&self, conduit_id: i32) -> Result<()> {
		if let Some(sync) = self.active_syncs.write().await.remove(&conduit_id) {
			info!("Canceling in-flight sync for conduit {}", conduit_id);
			if let Some(job_id) = sync.source_to_target.copy_job_id {
				let _ = self.library.jobs().cancel_job(job_id).await;
			}
			if let Some(job_id) = sync.source_to_target.delete_job_id {
				let _ = self.library.jobs().cancel_job(job_id).await;
			}
			if let Some(target) = sync.target_to_source {
				if let Some(job_id) = target.copy_job_id {
					let _ = self.library.jobs().cancel_job(job_id).await;
				}
				if let Some(job_id) = target.delete_job_id {
					let _ = self.library.jobs().cancel_job(job_id).await;
				}
			}
		}
		Ok(())
	}

	/// Trigger sync for a conduit
	pub async fn sync_now(&self, conduit_id: i32) -> Result<SyncHandle> {
		// Load conduit
		let conduit = self.conduit_manager.get_conduit(conduit_id).await?;

		if !conduit.enabled {
			return Err(anyhow::anyhow!("Conduit is disabled"));
		}

		// Library Sync First: refuse to operate while the metadata index may be
		// stale relative to peers. Operating on a stale index risks copying or
		// deleting the wrong files.
		if !self.is_library_sync_complete().await {
			return Err(anyhow::anyhow!(
				"Library sync is not complete; refusing to start file sync until the index is up to date"
			));
		}

		// Check if already syncing
		if self.active_syncs.read().await.contains_key(&conduit_id) {
			return Err(anyhow::anyhow!("Sync already in progress for this conduit"));
		}

		// Calculate sync operations
		info!("Calculating sync operations for conduit {}", conduit_id);
		let mut operations = self.resolver.calculate_operations(&conduit).await?;

		// Wire ConflictResolver
		let mut conflicts_resolved = 0;
		if !operations.conflicts.is_empty() {
			let strategy = conflict::ConflictStrategy::NewestWins; // TODO: make configurable
			let resolver = conflict::ConflictResolver::new(strategy);

			let mut remaining_conflicts = Vec::new();
			for conflict in operations.conflicts.drain(..) {
				match resolver.resolve(&conflict) {
					conflict::ConflictResolution::UseSource => {
						operations
							.source_to_target
							.to_copy
							.push(conflict.source_entry);
						conflicts_resolved += 1;
					}
					conflict::ConflictResolution::UseTarget => {
						if let Some(ref mut t_to_s) = operations.target_to_source {
							t_to_s.to_copy.push(conflict.target_entry);
							conflicts_resolved += 1;
						}
					}
					conflict::ConflictResolution::CreateConflictCopy { .. } => {
						// Blocker: current FileCopyJob cannot rename files during copy, need new Job API features
						remaining_conflicts.push(conflict);
					}
					conflict::ConflictResolution::PromptUser(c) => {
						remaining_conflicts.push(c);
					}
				}
			}
			operations.conflicts = remaining_conflicts;
		}

		let mode = sync_conduit::SyncMode::from_str(&conduit.sync_mode)
			.ok_or_else(|| anyhow::anyhow!("Invalid sync mode"))?;

		let copy_count = operations.source_to_target.to_copy.len()
			+ operations
				.target_to_source
				.as_ref()
				.map(|ops| ops.to_copy.len())
				.unwrap_or(0);
		let delete_count = operations.source_to_target.to_delete.len()
			+ operations
				.target_to_source
				.as_ref()
				.map(|ops| ops.to_delete.len())
				.unwrap_or(0);

		info!(
			"Sync plan for {:?} mode: {} to copy, {} to delete",
			mode, copy_count, delete_count
		);

		// If there's nothing to sync, mark as complete immediately
		if copy_count == 0 && delete_count == 0 && operations.conflicts.is_empty() {
			info!("No changes to sync for conduit {}", conduit_id);
			let generation = conduit.sync_generation + 1;
			self.emit_file_sync_started(conduit_id, generation);
			self.conduit_manager.update_after_sync(conduit_id).await?;
			self.emit_file_sync_completed(conduit_id, generation);
			return Ok(SyncHandle {
				conduit_id,
				generation,
				source_to_target: JobBatch {
					copy_job_id: None,
					delete_job_id: None,
				},
				target_to_source: None,
			});
		}

		// Create new generation
		let generation = self
			.conduit_manager
			.create_generation(conduit_id, conduit.sync_generation + 1, conflicts_resolved)
			.await?;
		self.emit_file_sync_started(conduit_id, generation.generation);

		// Dispatch source → target jobs
		let source_to_target = self
			.dispatch_job_batch(&conduit, &operations.source_to_target, "source → target")
			.await?;

		// Dispatch target → source jobs (bidirectional only)
		let target_to_source = if let Some(ref ops) = operations.target_to_source {
			Some(
				self.dispatch_job_batch(&conduit, ops, "target → source")
					.await?,
			)
		} else {
			None
		};

		// Track active sync
		let sync_op = SyncOperation {
			conduit_id,
			generation: generation.generation,
			generation_id: generation.id,
			started_at: chrono::Utc::now(),
			source_to_target: source_to_target.clone(),
			source_to_target_delete_ops: operations.source_to_target.clone(),
			target_to_source: target_to_source.clone(),
			target_to_source_delete_ops: operations.target_to_source.clone(),
		};

		self.active_syncs.write().await.insert(conduit_id, sync_op);
		self.emit_file_sync_progress(conduit_id, generation.generation, "queued");

		// Start monitoring background task
		let service = self.clone();
		tokio::spawn(async move {
			if let Err(e) = service.monitor_sync_internal(conduit_id).await {
				error!("Error monitoring sync {}: {}", conduit_id, e);
			}
		});

		Ok(SyncHandle {
			conduit_id,
			generation: generation.generation,
			source_to_target,
			target_to_source,
		})
	}

	/// Dispatch copy jobs for a single direction. Delete jobs are dispatched by
	/// the monitor after all copy jobs complete.
	async fn dispatch_job_batch(
		&self,
		_conduit: &sync_conduit::Model,
		operations: &DirectionalOps,
		direction: &str,
	) -> Result<JobBatch> {
		let jobs = self.library.jobs();

		let copy_job_id = if !operations.to_copy.is_empty() {
			info!(
				"{}: Dispatching copy job for {} files",
				direction,
				operations.to_copy.len()
			);

			// Extract device slug from first entry (simplified)
			// In production, this should be more robust
			let device_slug = crate::device::get_current_device_slug();

			// Create SdPaths from entries
			let source_paths: Vec<SdPath> = operations
				.to_copy
				.iter()
				.map(|e| e.to_sdpath(device_slug.clone()))
				.collect();

			let destination = operations
				.destination_root
				.clone()
				.map(SdPath::local)
				.ok_or_else(|| anyhow::anyhow!("Missing sync destination root"))?;

			let mut job = FileCopyJob::new(SdPathBatch::new(source_paths), destination);
			job = job.with_options(CopyOptions {
				overwrite: true, // File sync should overwrite
				..Default::default()
			});

			let handle = jobs.dispatch(job).await?;
			Some(handle.id())
		} else {
			None
		};

		Ok(JobBatch {
			copy_job_id,
			delete_job_id: None,
		})
	}

	async fn dispatch_delete_job(
		&self,
		operations: &DirectionalOps,
		direction: &str,
	) -> Result<Option<JobId>> {
		if operations.to_delete.is_empty() {
			return Ok(None);
		}

		info!(
			"{}: Dispatching delete job for {} files after copy completion",
			direction,
			operations.to_delete.len()
		);

		let device_slug = crate::device::get_current_device_slug();
		let paths: Vec<SdPath> = operations
			.to_delete
			.iter()
			.map(|e| e.to_sdpath(device_slug.clone()))
			.collect();

		let mut job = DeleteJob::new(SdPathBatch::new(paths), DeleteMode::Permanent);
		job.confirm_permanent = true;

		let handle = self.library.jobs().dispatch(job).await?;
		Ok(Some(handle.id()))
	}

	async fn record_delete_job_id(
		&self,
		conduit_id: i32,
		job_id: Option<JobId>,
		source_to_target: bool,
		direction: &str,
	) -> Result<()> {
		let Some(job_id) = job_id else {
			return Ok(());
		};

		let recorded = {
			let mut syncs = self.active_syncs.write().await;
			if let Some(sync) = syncs.get_mut(&conduit_id) {
				if source_to_target {
					sync.source_to_target.delete_job_id = Some(job_id);
					true
				} else if let Some(target_to_source) = sync.target_to_source.as_mut() {
					target_to_source.delete_job_id = Some(job_id);
					true
				} else {
					false
				}
			} else {
				false
			}
		};

		if !recorded {
			let _ = self.library.jobs().cancel_job(job_id).await;
			return Err(anyhow::anyhow!(
				"Sync was canceled before {} delete job could be tracked",
				direction
			));
		}

		Ok(())
	}

	async fn record_sync_failure(&self, conduit_id: i32, message: String) -> Result<()> {
		let generation = self
			.active_syncs
			.read()
			.await
			.get(&conduit_id)
			.map(|sync| sync.generation);
		let result = self
			.conduit_manager
			.record_sync_error(conduit_id, message.clone())
			.await;
		let _ = self.cancel_sync(conduit_id).await;
		self.emit_file_sync_failed(conduit_id, generation, message);
		result?;
		Ok(())
	}

	/// Monitor sync operation and update state when complete
	async fn monitor_sync_internal(&self, conduit_id: i32) -> Result<()> {
		// Get job batches
		let (
			source_to_target,
			target_to_source,
			source_to_target_delete_ops,
			target_to_source_delete_ops,
			generation,
			generation_id,
		) = {
			let syncs = self.active_syncs.read().await;
			let sync = syncs
				.get(&conduit_id)
				.ok_or_else(|| anyhow::anyhow!("Sync not found"))?;
			(
				sync.source_to_target.clone(),
				sync.target_to_source.clone(),
				sync.source_to_target_delete_ops.clone(),
				sync.target_to_source_delete_ops.clone(),
				sync.generation,
				sync.generation_id,
			)
		};

		// Phase 1: Wait for all copy jobs to complete
		self.emit_file_sync_progress(conduit_id, generation, "copying");
		info!(
			"Waiting for copy jobs to complete for conduit {}",
			conduit_id
		);

		let mut copy_job_ids = Vec::new();
		if let Some(id) = source_to_target.copy_job_id {
			copy_job_ids.push(id);
		}
		if let Some(ops) = &target_to_source {
			if let Some(id) = ops.copy_job_id {
				copy_job_ids.push(id);
			}
		}

		for job_id in copy_job_ids {
			if let Some(handle) = self.library.jobs().get_job(job_id).await {
				if let Err(err) = handle.wait().await {
					error!("Copy job {} failed: {}", job_id, err);
					self.record_sync_failure(conduit_id, format!("Copy job failed: {}", err))
						.await?;
					return Err(anyhow::anyhow!("Copy job failed: {}", err));
				}
			} else {
				let message = format!("Copy job {} not found", job_id);
				error!("{}", message);
				self.record_sync_failure(conduit_id, message.clone())
					.await?;
				return Err(anyhow::anyhow!(message));
			}
		}

		// Phase 2: Dispatch and wait for all delete jobs after copies have completed.
		self.emit_file_sync_progress(conduit_id, generation, "deleting");
		let source_delete_job_id = match self
			.dispatch_delete_job(&source_to_target_delete_ops, "source → target")
			.await
		{
			Ok(job_id) => job_id,
			Err(err) => {
				self.record_sync_failure(
					conduit_id,
					format!("Failed to dispatch source → target delete job: {}", err),
				)
				.await?;
				return Err(err);
			}
		};
		self.record_delete_job_id(conduit_id, source_delete_job_id, true, "source → target")
			.await?;

		let target_delete_job_id = if let Some(ops) = &target_to_source_delete_ops {
			match self.dispatch_delete_job(ops, "target → source").await {
				Ok(job_id) => job_id,
				Err(err) => {
					self.record_sync_failure(
						conduit_id,
						format!("Failed to dispatch target → source delete job: {}", err),
					)
					.await?;
					return Err(err);
				}
			}
		} else {
			None
		};
		self.record_delete_job_id(conduit_id, target_delete_job_id, false, "target → source")
			.await?;

		info!(
			"Waiting for delete jobs to complete for conduit {}",
			conduit_id
		);

		let mut delete_job_ids = Vec::new();
		if let Some(id) = source_delete_job_id {
			delete_job_ids.push(id);
		}
		if let Some(id) = target_delete_job_id {
			delete_job_ids.push(id);
		}

		for job_id in delete_job_ids {
			if let Some(handle) = self.library.jobs().get_job(job_id).await {
				if let Err(err) = handle.wait().await {
					error!("Delete job {} failed: {}", job_id, err);
					self.record_sync_failure(conduit_id, format!("Delete job failed: {}", err))
						.await?;
					return Err(anyhow::anyhow!("Delete job failed: {}", err));
				}
			} else {
				let message = format!("Delete job {} not found", job_id);
				error!("{}", message);
				self.record_sync_failure(conduit_id, message.clone())
					.await?;
				return Err(anyhow::anyhow!(message));
			}
		}

		// Phase 3: Mark sync as complete
		self.emit_file_sync_progress(conduit_id, generation, "finalizing");
		let files_copied = source_to_target_delete_ops.to_copy.len()
			+ target_to_source_delete_ops
				.as_ref()
				.map(|ops| ops.to_copy.len())
				.unwrap_or(0);
		let files_deleted = source_to_target_delete_ops.to_delete.len()
			+ target_to_source_delete_ops
				.as_ref()
				.map(|ops| ops.to_delete.len())
				.unwrap_or(0);
		if let Err(err) = self
			.conduit_manager
			.record_generation_counts(generation_id, files_copied as i32, files_deleted as i32)
			.await
		{
			warn!(
				"Failed to record generation counts for conduit {}: {}",
				conduit_id, err
			);
		}
		if let Err(err) = self
			.conduit_manager
			.complete_generation(generation_id)
			.await
		{
			self.record_sync_failure(
				conduit_id,
				format!("Failed to complete generation: {}", err),
			)
			.await?;
			return Err(err);
		}
		if let Err(err) = self.conduit_manager.update_after_sync(conduit_id).await {
			self.record_sync_failure(
				conduit_id,
				format!("Failed to update conduit after sync: {}", err),
			)
			.await?;
			return Err(err);
		}

		info!(
			"Sync operations completed for conduit {}, starting verification",
			conduit_id
		);

		// Phase 4: Trust Watcher verification
		self.emit_file_sync_progress(conduit_id, generation, "verifying");
		if let Err(err) = self
			.complete_sync_with_verification(conduit_id, generation_id)
			.await
		{
			warn!(
				"Verification failed for conduit {} generation {}: {}",
				conduit_id, generation, err
			);
		}

		// Phase 5: Remove from active syncs
		self.active_syncs.write().await.remove(&conduit_id);
		self.emit_file_sync_completed(conduit_id, generation);

		info!(
			"Sync fully completed and verified for conduit {}",
			conduit_id
		);

		Ok(())
	}

	/// Trust Watcher verification flow.
	///
	/// After the sync jobs complete: mark the generation as waiting on the
	/// watcher, refresh the index for both endpoints (a complete re-scan — the
	/// same convergence the filesystem watcher provides), wait for a library
	/// sync round, then re-run sync resolution. If no operations remain the
	/// generation is `verified`; otherwise it is marked `failed:<reason>` so a
	/// later sync round can converge.
	async fn complete_sync_with_verification(
		&self,
		conduit_id: i32,
		generation_id: i32,
	) -> Result<()> {
		let conduit = self.conduit_manager.get_conduit(conduit_id).await?;

		// Wait for the watcher/index to settle. The re-scan inside
		// verify_conduit below is the index refresh the watcher would perform.
		self.conduit_manager
			.update_verification_status(generation_id, "waiting_watcher")
			.await?;

		// Wait for a library sync round so remote metadata is current.
		self.conduit_manager
			.update_verification_status(generation_id, "waiting_library_sync")
			.await?;
		if !self.wait_for_library_sync().await {
			let status = sync_generation::VerificationStatus::failed("library_sync_timeout");
			self.conduit_manager
				.update_verification_status(generation_id, &status)
				.await?;
			return Err(anyhow::anyhow!(
				"Library sync did not complete within the verification window"
			));
		}

		// Re-run sync resolution against a fresh complete scan.
		match self.resolver.verify_conduit(&conduit).await {
			Ok(operations) if operations.is_converged() => {
				self.conduit_manager
					.update_verification_status(generation_id, "verified")
					.await?;
				Ok(())
			}
			Ok(operations) => {
				let remaining = operations.source_to_target.to_copy.len()
					+ operations.source_to_target.to_delete.len()
					+ operations
						.target_to_source
						.as_ref()
						.map(|ops| ops.to_copy.len() + ops.to_delete.len())
						.unwrap_or(0) + operations.conflicts.len();
				let status = sync_generation::VerificationStatus::failed(&format!(
					"{}_operations_remaining",
					remaining
				));
				self.conduit_manager
					.update_verification_status(generation_id, &status)
					.await?;
				Err(anyhow::anyhow!(
					"Verification found {} remaining operations",
					remaining
				))
			}
			Err(err) => {
				let status = sync_generation::VerificationStatus::failed("resolution_error");
				self.conduit_manager
					.update_verification_status(generation_id, &status)
					.await?;
				Err(err)
			}
		}
	}

	/// Poll until library sync is complete, up to a bounded window.
	async fn wait_for_library_sync(&self) -> bool {
		const MAX_ATTEMPTS: u32 = 60;
		const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

		for _ in 0..MAX_ATTEMPTS {
			if self.is_library_sync_complete().await {
				return true;
			}
			tokio::time::sleep(POLL_INTERVAL).await;
		}
		false
	}

	/// Whether the library's metadata sync is complete enough to trust the
	/// index. Standalone libraries (no sync service) are always complete;
	/// libraries mid-backfill or catching up on buffered updates are not.
	pub async fn is_library_sync_complete(&self) -> bool {
		if let Some(ready) = *self.library_sync_ready_override.read().await {
			return ready;
		}

		match self.library.sync_service() {
			Some(sync_service) => {
				let state = sync_service.peer_sync().state().await;
				!state.should_buffer()
			}
			None => true,
		}
	}

	/// Force the library-sync readiness gate open (`Some(true)`), closed
	/// (`Some(false)`), or defer to the real sync state (`None`).
	pub async fn set_library_sync_ready_override(&self, ready: Option<bool>) {
		*self.library_sync_ready_override.write().await = ready;
	}

	/// Check if a conduit is currently syncing
	pub async fn is_syncing(&self, conduit_id: i32) -> bool {
		self.active_syncs.read().await.contains_key(&conduit_id)
	}

	/// Get the conduit manager
	pub fn conduit_manager(&self) -> &Arc<ConduitManager> {
		&self.conduit_manager
	}

	/// Get the generation history queries
	pub fn history(&self) -> &Arc<SyncHistory> {
		&self.history
	}

	fn emit_file_sync_started(&self, conduit_id: i32, generation: i64) {
		self.library.event_bus().emit(Event::FileSyncStarted {
			library_id: self.library.id(),
			conduit_id,
			generation,
		});
	}

	fn emit_file_sync_progress(&self, conduit_id: i32, generation: i64, phase: &str) {
		self.library.event_bus().emit(Event::FileSyncProgress {
			library_id: self.library.id(),
			conduit_id,
			generation,
			phase: phase.to_string(),
		});
	}

	fn emit_file_sync_completed(&self, conduit_id: i32, generation: i64) {
		self.library.event_bus().emit(Event::FileSyncCompleted {
			library_id: self.library.id(),
			conduit_id,
			generation,
		});
	}

	fn emit_file_sync_failed(&self, conduit_id: i32, generation: Option<i64>, error: String) {
		self.library.event_bus().emit(Event::FileSyncFailed {
			library_id: self.library.id(),
			conduit_id,
			generation,
			error,
		});
	}
}

impl Clone for FileSyncService {
	fn clone(&self) -> Self {
		Self {
			library: self.library.clone(),
			conduit_manager: self.conduit_manager.clone(),
			resolver: self.resolver.clone(),
			history: self.history.clone(),
			active_syncs: self.active_syncs.clone(),
			library_sync_ready_override: self.library_sync_ready_override.clone(),
		}
	}
}

/// Handle to a running sync operation
#[derive(Debug, Clone)]
pub struct SyncHandle {
	pub conduit_id: i32,
	pub generation: i64,
	pub source_to_target: JobBatch,
	pub target_to_source: Option<JobBatch>,
}
