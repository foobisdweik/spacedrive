use crate::{
	context::CoreContext,
	infra::{
		action::{error::ActionError, LibraryAction},
		api::SessionContext,
		db::entities::{sync_conduit, sync_generation},
		event::Event,
		job::types::JobId,
		query::{LibraryQuery, QueryError, QueryResult},
	},
	library::Library,
	service::file_sync::{FileSyncService, SyncHandle},
};
use chrono::{DateTime, Utc};
use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CreateConduitInput {
	pub source_entry_id: i32,
	pub target_entry_id: i32,
	pub sync_mode: String,
	pub schedule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UpdateConduitInput {
	pub conduit_id: i32,
	pub sync_mode: Option<String>,
	pub enabled: Option<bool>,
	pub schedule: Option<String>,
	pub use_index_rules: Option<bool>,
	pub index_mode_override: Option<Option<String>>,
	pub parallel_transfers: Option<i32>,
	pub bandwidth_limit_mbps: Option<Option<i32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DeleteConduitInput {
	pub conduit_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncNowInput {
	pub conduit_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PauseSyncInput {
	pub conduit_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResumeSyncInput {
	pub conduit_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GetSyncStatusInput {
	pub conduit_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GetSyncProgressInput {
	pub conduit_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GetConflictsInput {
	pub conduit_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncHistoryInput {
	pub conduit_id: i32,
	pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncConduitResponse {
	pub id: i32,
	pub uuid: Uuid,
	pub source_entry_id: i32,
	pub target_entry_id: i32,
	pub sync_mode: String,
	pub enabled: bool,
	pub schedule: String,
	pub use_index_rules: bool,
	pub index_mode_override: Option<String>,
	pub parallel_transfers: i32,
	pub bandwidth_limit_mbps: Option<i32>,
	pub last_sync_completed_at: Option<DateTime<Utc>>,
	pub sync_generation: i64,
	pub last_sync_error: Option<String>,
	pub total_syncs: i64,
	pub files_synced: i64,
	pub bytes_transferred: i64,
	pub created_at: DateTime<Utc>,
	pub updated_at: DateTime<Utc>,
	pub is_syncing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncNowOutput {
	pub conduit_id: i32,
	pub generation: i64,
	pub source_to_target: JobBatchResponse,
	pub target_to_source: Option<JobBatchResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct JobBatchResponse {
	pub copy_job_id: Option<JobId>,
	pub delete_job_id: Option<JobId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncStatusResponse {
	pub conduit_id: i32,
	pub is_syncing: bool,
	pub enabled: bool,
	pub sync_generation: i64,
	pub last_sync_completed_at: Option<DateTime<Utc>>,
	pub last_sync_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncProgressResponse {
	pub conduit_id: i32,
	pub is_syncing: bool,
	pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncGenerationResponse {
	pub id: i32,
	pub conduit_id: i32,
	pub generation: i64,
	pub started_at: DateTime<Utc>,
	pub completed_at: Option<DateTime<Utc>>,
	pub files_copied: i32,
	pub files_deleted: i32,
	pub conflicts_resolved: i32,
	pub bytes_transferred: i64,
	pub errors_encountered: i32,
	pub verified_at: Option<DateTime<Utc>>,
	pub verification_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConflictListOutput {
	pub conduit_id: i32,
	pub conflicts: Vec<SyncConflictResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncConflictResponse {
	pub relative_path: String,
	pub conflict_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateConduitAction {
	input: CreateConduitInput,
}

impl LibraryAction for CreateConduitAction {
	type Input = CreateConduitInput;
	type Output = SyncConduitResponse;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		parse_sync_mode(&input.sync_mode).map_err(|error| error.to_string())?;
		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let service = file_sync_service_for_action(&library)?;
		let mode = parse_sync_mode(&self.input.sync_mode)?;
		let conduit = service
			.conduit_manager()
			.create_conduit(
				self.input.source_entry_id,
				self.input.target_entry_id,
				mode,
				self.input.schedule,
			)
			.await
			.map_err(to_action_error)?;

		emit_conduit_changed(&library, conduit.id, "created");
		Ok(conduit_response(conduit, false))
	}

	fn action_kind(&self) -> &'static str {
		"file_sync.conduit.create"
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConduitAction {
	input: UpdateConduitInput,
}

impl LibraryAction for UpdateConduitAction {
	type Input = UpdateConduitInput;
	type Output = SyncConduitResponse;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		if let Some(sync_mode) = &input.sync_mode {
			parse_sync_mode(sync_mode).map_err(|error| error.to_string())?;
		}
		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let service = file_sync_service_for_action(&library)?;
		let sync_mode = self
			.input
			.sync_mode
			.as_deref()
			.map(parse_sync_mode)
			.transpose()?;

		let conduit = service
			.conduit_manager()
			.update_conduit(
				self.input.conduit_id,
				sync_mode,
				self.input.enabled,
				self.input.schedule,
				self.input.use_index_rules,
				self.input.index_mode_override,
				self.input.parallel_transfers,
				self.input.bandwidth_limit_mbps,
			)
			.await
			.map_err(to_action_error)?;

		let is_syncing = service.is_syncing(conduit.id).await;
		emit_conduit_changed(&library, conduit.id, "updated");
		Ok(conduit_response(conduit, is_syncing))
	}

	fn action_kind(&self) -> &'static str {
		"file_sync.conduit.update"
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteConduitAction {
	input: DeleteConduitInput,
}

impl LibraryAction for DeleteConduitAction {
	type Input = DeleteConduitInput;
	type Output = ();

	fn from_input(input: Self::Input) -> Result<Self, String> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		file_sync_service_for_action(&library)?
			.conduit_manager()
			.delete_conduit(self.input.conduit_id)
			.await
			.map_err(to_action_error)?;
		emit_conduit_changed(&library, self.input.conduit_id, "deleted");
		Ok(())
	}

	fn action_kind(&self) -> &'static str {
		"file_sync.conduit.delete"
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncNowAction {
	input: SyncNowInput,
}

impl LibraryAction for SyncNowAction {
	type Input = SyncNowInput;
	type Output = SyncNowOutput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let handle = file_sync_service_for_action(&library)?
			.sync_now(self.input.conduit_id)
			.await
			.map_err(to_action_error)?;

		Ok(sync_now_output(handle))
	}

	fn action_kind(&self) -> &'static str {
		"file_sync.sync.now"
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseSyncAction {
	input: PauseSyncInput,
}

impl LibraryAction for PauseSyncAction {
	type Input = PauseSyncInput;
	type Output = SyncStatusResponse;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let service = file_sync_service_for_action(&library)?;
		service
			.conduit_manager()
			.set_enabled(self.input.conduit_id, false)
			.await
			.map_err(to_action_error)?;
		service
			.cancel_sync(self.input.conduit_id)
			.await
			.map_err(to_action_error)?;
		emit_conduit_changed(&library, self.input.conduit_id, "paused");
		sync_status_for_action(&service, self.input.conduit_id).await
	}

	fn action_kind(&self) -> &'static str {
		"file_sync.sync.pause"
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeSyncAction {
	input: ResumeSyncInput,
}

impl LibraryAction for ResumeSyncAction {
	type Input = ResumeSyncInput;
	type Output = SyncStatusResponse;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let service = file_sync_service_for_action(&library)?;
		service
			.conduit_manager()
			.set_enabled(self.input.conduit_id, true)
			.await
			.map_err(to_action_error)?;
		emit_conduit_changed(&library, self.input.conduit_id, "resumed");
		sync_status_for_action(&service, self.input.conduit_id).await
	}

	fn action_kind(&self) -> &'static str {
		"file_sync.sync.resume"
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ListConduitsQuery;

impl LibraryQuery for ListConduitsQuery {
	type Input = EmptyInput;
	type Output = Vec<SyncConduitResponse>;

	fn from_input(_input: Self::Input) -> QueryResult<Self> {
		Ok(Self)
	}

	async fn execute(
		self,
		context: Arc<CoreContext>,
		session: SessionContext,
	) -> QueryResult<Self::Output> {
		let service = file_sync_service_for_query(&context, &session).await?;
		let conduits = service
			.conduit_manager()
			.list_all()
			.await
			.map_err(to_query_error)?;
		let mut output = Vec::with_capacity(conduits.len());

		for conduit in conduits {
			let is_syncing = service.is_syncing(conduit.id).await;
			output.push(conduit_response(conduit, is_syncing));
		}

		Ok(output)
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GetSyncStatusQuery {
	input: GetSyncStatusInput,
}

impl LibraryQuery for GetSyncStatusQuery {
	type Input = GetSyncStatusInput;
	type Output = SyncStatusResponse;

	fn from_input(input: Self::Input) -> QueryResult<Self> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		context: Arc<CoreContext>,
		session: SessionContext,
	) -> QueryResult<Self::Output> {
		let service = file_sync_service_for_query(&context, &session).await?;
		sync_status_for_query(&service, self.input.conduit_id).await
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GetSyncProgressQuery {
	input: GetSyncProgressInput,
}

impl LibraryQuery for GetSyncProgressQuery {
	type Input = GetSyncProgressInput;
	type Output = SyncProgressResponse;

	fn from_input(input: Self::Input) -> QueryResult<Self> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		context: Arc<CoreContext>,
		session: SessionContext,
	) -> QueryResult<Self::Output> {
		let service = file_sync_service_for_query(&context, &session).await?;
		let is_syncing = service.is_syncing(self.input.conduit_id).await;

		Ok(SyncProgressResponse {
			conduit_id: self.input.conduit_id,
			is_syncing,
			phase: if is_syncing { "syncing" } else { "idle" }.to_string(),
		})
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GetSyncHistoryQuery {
	input: SyncHistoryInput,
}

impl LibraryQuery for GetSyncHistoryQuery {
	type Input = SyncHistoryInput;
	type Output = Vec<SyncGenerationResponse>;

	fn from_input(input: Self::Input) -> QueryResult<Self> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		context: Arc<CoreContext>,
		session: SessionContext,
	) -> QueryResult<Self::Output> {
		let service = file_sync_service_for_query(&context, &session).await?;
		let generations = service
			.conduit_manager()
			.list_generations(
				self.input.conduit_id,
				self.input.limit.unwrap_or(50).min(500),
			)
			.await
			.map_err(to_query_error)?;

		Ok(generations.into_iter().map(generation_response).collect())
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GetConflictsQuery {
	input: GetConflictsInput,
}

impl LibraryQuery for GetConflictsQuery {
	type Input = GetConflictsInput;
	type Output = ConflictListOutput;

	fn from_input(input: Self::Input) -> QueryResult<Self> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		_context: Arc<CoreContext>,
		_session: SessionContext,
	) -> QueryResult<Self::Output> {
		Ok(ConflictListOutput {
			conduit_id: self.input.conduit_id,
			conflicts: Vec::new(),
		})
	}
}

fn parse_sync_mode(mode: &str) -> Result<sync_conduit::SyncMode, ActionError> {
	sync_conduit::SyncMode::from_str(mode).ok_or_else(|| ActionError::Validation {
		field: "sync_mode".to_string(),
		message: "Expected one of: mirror, bidirectional, selective".to_string(),
	})
}

fn file_sync_service_for_action(
	library: &Arc<Library>,
) -> Result<Arc<FileSyncService>, ActionError> {
	library
		.init_file_sync_service()
		.map_err(ActionError::from)?;
	library
		.file_sync_service()
		.cloned()
		.ok_or_else(|| ActionError::Internal("File sync service is not initialized".to_string()))
}

async fn file_sync_service_for_query(
	context: &Arc<CoreContext>,
	session: &SessionContext,
) -> QueryResult<Arc<FileSyncService>> {
	let library_id = session
		.current_library_id
		.ok_or_else(|| QueryError::Internal("No library selected".to_string()))?;
	let library = context
		.libraries()
		.await
		.get_library(library_id)
		.await
		.ok_or_else(|| QueryError::LibraryNotFound(library_id))?;

	library.init_file_sync_service().map_err(QueryError::from)?;
	library
		.file_sync_service()
		.cloned()
		.ok_or_else(|| QueryError::Internal("File sync service is not initialized".to_string()))
}

async fn sync_status_for_action(
	service: &Arc<FileSyncService>,
	conduit_id: i32,
) -> Result<SyncStatusResponse, ActionError> {
	let conduit = service
		.conduit_manager()
		.get_conduit(conduit_id)
		.await
		.map_err(to_action_error)?;
	let is_syncing = service.is_syncing(conduit_id).await;

	Ok(sync_status_response(conduit, is_syncing))
}

async fn sync_status_for_query(
	service: &Arc<FileSyncService>,
	conduit_id: i32,
) -> QueryResult<SyncStatusResponse> {
	let conduit = service
		.conduit_manager()
		.get_conduit(conduit_id)
		.await
		.map_err(to_query_error)?;
	let is_syncing = service.is_syncing(conduit_id).await;

	Ok(sync_status_response(conduit, is_syncing))
}

fn conduit_response(conduit: sync_conduit::Model, is_syncing: bool) -> SyncConduitResponse {
	SyncConduitResponse {
		id: conduit.id,
		uuid: conduit.uuid,
		source_entry_id: conduit.source_entry_id,
		target_entry_id: conduit.target_entry_id,
		sync_mode: conduit.sync_mode,
		enabled: conduit.enabled,
		schedule: conduit.schedule,
		use_index_rules: conduit.use_index_rules,
		index_mode_override: conduit.index_mode_override,
		parallel_transfers: conduit.parallel_transfers,
		bandwidth_limit_mbps: conduit.bandwidth_limit_mbps,
		last_sync_completed_at: conduit.last_sync_completed_at,
		sync_generation: conduit.sync_generation,
		last_sync_error: conduit.last_sync_error,
		total_syncs: conduit.total_syncs,
		files_synced: conduit.files_synced,
		bytes_transferred: conduit.bytes_transferred,
		created_at: conduit.created_at,
		updated_at: conduit.updated_at,
		is_syncing,
	}
}

fn sync_now_output(handle: SyncHandle) -> SyncNowOutput {
	SyncNowOutput {
		conduit_id: handle.conduit_id,
		generation: handle.generation,
		source_to_target: job_batch_response(handle.source_to_target),
		target_to_source: handle.target_to_source.map(job_batch_response),
	}
}

fn job_batch_response(batch: crate::service::file_sync::JobBatch) -> JobBatchResponse {
	JobBatchResponse {
		copy_job_id: batch.copy_job_id,
		delete_job_id: batch.delete_job_id,
	}
}

fn sync_status_response(conduit: sync_conduit::Model, is_syncing: bool) -> SyncStatusResponse {
	SyncStatusResponse {
		conduit_id: conduit.id,
		is_syncing,
		enabled: conduit.enabled,
		sync_generation: conduit.sync_generation,
		last_sync_completed_at: conduit.last_sync_completed_at,
		last_sync_error: conduit.last_sync_error,
	}
}

fn generation_response(generation: sync_generation::Model) -> SyncGenerationResponse {
	SyncGenerationResponse {
		id: generation.id,
		conduit_id: generation.conduit_id,
		generation: generation.generation,
		started_at: generation.started_at,
		completed_at: generation.completed_at,
		files_copied: generation.files_copied,
		files_deleted: generation.files_deleted,
		conflicts_resolved: generation.conflicts_resolved,
		bytes_transferred: generation.bytes_transferred,
		errors_encountered: generation.errors_encountered,
		verified_at: generation.verified_at,
		verification_status: generation.verification_status,
	}
}

fn emit_conduit_changed(library: &Library, conduit_id: i32, change_type: &str) {
	library.event_bus().emit(Event::FileSyncConduitChanged {
		library_id: library.id(),
		conduit_id,
		change_type: change_type.to_string(),
	});
}

fn to_action_error(error: anyhow::Error) -> ActionError {
	ActionError::Internal(error.to_string())
}

fn to_query_error(error: anyhow::Error) -> QueryError {
	QueryError::Internal(error.to_string())
}

crate::register_library_action!(CreateConduitAction, "file_sync.conduit.create");
crate::register_library_action!(UpdateConduitAction, "file_sync.conduit.update");
crate::register_library_action!(DeleteConduitAction, "file_sync.conduit.delete");
crate::register_library_action!(SyncNowAction, "file_sync.sync.now");
crate::register_library_action!(PauseSyncAction, "file_sync.sync.pause");
crate::register_library_action!(ResumeSyncAction, "file_sync.sync.resume");
crate::register_library_query!(ListConduitsQuery, "file_sync.conduit.list");
crate::register_library_query!(GetSyncStatusQuery, "file_sync.status.get");
crate::register_library_query!(GetSyncProgressQuery, "file_sync.status.progress");
crate::register_library_query!(GetSyncHistoryQuery, "file_sync.history.list");
crate::register_library_query!(GetConflictsQuery, "file_sync.conflicts.list");
