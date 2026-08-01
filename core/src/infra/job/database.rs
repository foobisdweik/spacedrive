//! Job database schema and operations
//! This is the database for the job manager, not the global library database.
//! It is used to store the job history and checkpoints with serializable data for resuming jobs.
//! The job database is not synced between devices.
//! Jobs must be dispatched by the action system if initiated by the user.

use super::{
	error::{JobError, JobResult},
	progress::Progress,
	types::{JobId, JobMetrics, JobStatus},
};
use chrono::{DateTime, Utc};
use sea_orm::{
	entity::prelude::*,
	sea_query::{Expr, Query},
	ActiveModelTrait,
	ActiveValue::Set,
	ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait, QueryFilter, Schema,
	TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;

pub mod jobs {
	use super::*;

	/// Job record in the database
	#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
	#[sea_orm(table_name = "jobs")]
	pub struct Model {
		#[sea_orm(primary_key, auto_increment = false)]
		pub id: String,
		pub name: String,
		pub state: Vec<u8>,
		pub status: String,
		pub priority: i32,

		// Progress tracking
		pub progress_type: Option<String>,
		pub progress_data: Option<Vec<u8>>,

		// Relationships
		pub parent_job_id: Option<String>,

		// Timestamps
		pub created_at: DateTime<Utc>,
		pub started_at: Option<DateTime<Utc>>,
		pub completed_at: Option<DateTime<Utc>>,
		pub paused_at: Option<DateTime<Utc>>,

		// Error tracking
		pub error_message: Option<String>,
		pub warnings: Option<JsonValue>,
		pub non_critical_errors: Option<JsonValue>,

		// Metrics
		pub metrics: Option<Vec<u8>>,

		// Action context
		/// Serialized ActionContext that spawned this job
		pub action_context: Option<Vec<u8>>,

		/// Action type for efficient querying
		pub action_type: Option<String>,
	}

	#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
	pub enum Relation {}

	impl ActiveModelBehavior for ActiveModel {}
}

pub mod history {
	use super::*;

	/// Job history record
	#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
	#[sea_orm(table_name = "job_history")]
	pub struct Model {
		#[sea_orm(primary_key, auto_increment = false)]
		pub id: String,
		pub name: String,
		pub status: String,
		pub started_at: DateTime<Utc>,
		pub completed_at: DateTime<Utc>,
		pub duration_ms: i64,
		pub output: Option<Vec<u8>>,
		pub metrics: Option<Vec<u8>>,
	}

	#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
	pub enum Relation {}

	impl ActiveModelBehavior for ActiveModel {}
}

pub mod checkpoint {
	use super::*;

	/// Job checkpoint record
	#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
	#[sea_orm(table_name = "job_checkpoints")]
	pub struct Model {
		#[sea_orm(primary_key, auto_increment = false)]
		pub job_id: String,
		pub checkpoint_data: Vec<u8>,
		pub created_at: DateTime<Utc>,
	}

	#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
	pub enum Relation {}

	impl ActiveModelBehavior for ActiveModel {}
}

/// Initialize job database
pub async fn init_database(db_file_path: &Path) -> JobResult<DatabaseConnection> {
	// Ensure the parent directory exists
	if let Some(parent) = db_file_path.parent() {
		tokio::fs::create_dir_all(parent).await?;
	}

	let db_url = format!("sqlite://{}?mode=rwc", db_file_path.display());

	let db = sea_orm::Database::connect(&db_url).await?;

	// Create tables
	create_tables(&db).await?;

	Ok(db)
}

/// Create job tables
async fn create_tables(db: &DatabaseConnection) -> JobResult<()> {
	let schema = Schema::new(DbBackend::Sqlite);

	// Create jobs table if not exists
	let mut jobs_statement = schema.create_table_from_entity(jobs::Entity);
	jobs_statement.if_not_exists();
	db.execute(db.get_database_backend().build(&jobs_statement))
		.await?;

	// Create history table if not exists
	let mut history_statement = schema.create_table_from_entity(history::Entity);
	history_statement.if_not_exists();
	db.execute(db.get_database_backend().build(&history_statement))
		.await?;

	// Create checkpoint table if not exists
	let mut checkpoint_statement = schema.create_table_from_entity(checkpoint::Entity);
	checkpoint_statement.if_not_exists();
	db.execute(db.get_database_backend().build(&checkpoint_statement))
		.await?;

	Ok(())
}

/// Job database operations
pub struct JobDb {
	conn: DatabaseConnection,
}

impl JobDb {
	pub fn new(conn: DatabaseConnection) -> Self {
		Self { conn }
	}

	pub fn conn(&self) -> &DatabaseConnection {
		&self.conn
	}

	/// Get all queued jobs
	pub async fn get_queued_jobs(&self) -> JobResult<Vec<jobs::Model>> {
		jobs::Entity::find()
			.filter(jobs::Column::Status.eq(JobStatus::Queued.to_string()))
			.all(&self.conn)
			.await
			.map_err(Into::into)
	}

	/// Get a job by ID
	pub async fn get_job(&self, id: JobId) -> JobResult<Option<jobs::Model>> {
		jobs::Entity::find_by_id(id.to_string())
			.one(&self.conn)
			.await
			.map_err(Into::into)
	}

	/// Update job status
	pub async fn update_status(&self, id: JobId, status: JobStatus) -> JobResult<()> {
		let mut job = jobs::ActiveModel {
			id: Set(id.to_string()),
			status: Set(status.to_string()),
			..Default::default()
		};

		// Update timestamps based on status
		match status {
			JobStatus::Running => {
				job.started_at = Set(Some(Utc::now()));
			}
			JobStatus::Paused => {
				job.paused_at = Set(Some(Utc::now()));
			}
			JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => {
				job.completed_at = Set(Some(Utc::now()));
			}
			_ => {}
		}

		job.update(&self.conn).await?;
		Ok(())
	}

	/// Update job progress in database
	pub async fn update_progress(&self, job_id: JobId, progress: &Progress) -> JobResult<()> {
		let progress_data = rmp_serde::to_vec(progress).map_err(|e| JobError::serialization(e))?;

		let mut job = jobs::ActiveModel {
			id: Set(job_id.to_string()),
			progress_data: Set(Some(progress_data)),
			..Default::default()
		};

		job.update(&self.conn).await?;
		Ok(())
	}

	/// Update job status and optionally progress atomically
	pub async fn update_status_and_progress(
		&self,
		job_id: JobId,
		status: JobStatus,
		progress: Option<&Progress>,
		error_message: Option<String>,
	) -> JobResult<()> {
		// Use update query builder for partial updates
		let mut update = jobs::Entity::update_many()
			.filter(jobs::Column::Id.eq(job_id.to_string()))
			.col_expr(jobs::Column::Status, Expr::value(status.to_string()));

		// Update progress if provided
		if let Some(prog) = progress {
			let progress_data = rmp_serde::to_vec(prog).map_err(|e| JobError::serialization(e))?;
			update = update.col_expr(jobs::Column::ProgressData, Expr::value(progress_data));
		}

		// Update error message if provided
		if let Some(err_msg) = error_message {
			update = update.col_expr(jobs::Column::ErrorMessage, Expr::value(err_msg));
		}

		// Update timestamps based on status
		let now = Utc::now();
		match status {
			JobStatus::Running => {
				update = update.col_expr(jobs::Column::StartedAt, Expr::value(now));
			}
			JobStatus::Paused => {
				update = update.col_expr(jobs::Column::PausedAt, Expr::value(now));
			}
			JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => {
				update = update.col_expr(jobs::Column::CompletedAt, Expr::value(now));
			}
			_ => {}
		}

		let result = update.exec(&self.conn).await?;

		if result.rows_affected == 0 {
			return Err(JobError::NotFound(format!(
				"Job {} not found or update failed",
				job_id
			)));
		}

		Ok(())
	}

	/// Clean up old job history
	pub async fn cleanup_history(&self, older_than: DateTime<Utc>) -> JobResult<u64> {
		let result = history::Entity::delete_many()
			.filter(history::Column::CompletedAt.lt(older_than))
			.exec(&self.conn)
			.await?;

		Ok(result.rows_affected)
	}

	/// Delete jobs that have reached a terminal state.
	pub async fn clear_finished_jobs(&self) -> JobResult<u64> {
		let txn = self.conn.begin().await?;
		let terminal_jobs = jobs::Entity::find()
			.filter(jobs::Column::Status.is_in([
				JobStatus::Completed.to_string(),
				JobStatus::Failed.to_string(),
				JobStatus::Cancelled.to_string(),
			]))
			.all(&txn)
			.await?;

		if terminal_jobs.is_empty() {
			txn.commit().await?;
			return Ok(0);
		}

		let terminal_job_ids = terminal_jobs
			.into_iter()
			.map(|job| job.id)
			.collect::<Vec<_>>();

		let mut rows_affected = 0;
		for job_ids in terminal_job_ids.chunks(900) {
			checkpoint::Entity::delete_many()
				.filter(checkpoint::Column::JobId.is_in(job_ids.iter().cloned()))
				.exec(&txn)
				.await?;

			rows_affected += jobs::Entity::delete_many()
				.filter(jobs::Column::Id.is_in(job_ids.iter().cloned()))
				.exec(&txn)
				.await?
				.rows_affected;
		}

		txn.commit().await?;

		Ok(rows_affected)
	}
}

#[cfg(test)]
mod tests {
	use sea_orm::{ActiveModelTrait, Database, EntityTrait};

	use super::*;

	async fn insert_job(conn: &DatabaseConnection, status: JobStatus) {
		let now = Utc::now();
		jobs::ActiveModel {
			id: Set(status.to_string()),
			name: Set("test".to_string()),
			state: Set(Vec::new()),
			status: Set(status.to_string()),
			priority: Set(0),
			progress_type: Set(None),
			progress_data: Set(None),
			parent_job_id: Set(None),
			created_at: Set(now),
			started_at: Set(None),
			completed_at: Set(status.is_terminal().then_some(now)),
			paused_at: Set(None),
			error_message: Set(None),
			warnings: Set(None),
			non_critical_errors: Set(None),
			metrics: Set(None),
			action_context: Set(None),
			action_type: Set(None),
		}
		.insert(conn)
		.await
		.unwrap();
	}

	async fn insert_checkpoint(conn: &DatabaseConnection, status: JobStatus) {
		checkpoint::ActiveModel {
			job_id: Set(status.to_string()),
			checkpoint_data: Set(vec![1, 2, 3]),
			created_at: Set(Utc::now()),
		}
		.insert(conn)
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn clear_finished_jobs_preserves_active_jobs() {
		let conn = Database::connect("sqlite::memory:").await.unwrap();
		create_tables(&conn).await.unwrap();

		for status in [
			JobStatus::Queued,
			JobStatus::Running,
			JobStatus::Paused,
			JobStatus::Completed,
			JobStatus::Failed,
			JobStatus::Cancelled,
		] {
			insert_job(&conn, status).await;
			insert_checkpoint(&conn, status).await;
		}

		let db = JobDb::new(conn);
		assert_eq!(db.clear_finished_jobs().await.unwrap(), 3);

		let remaining = jobs::Entity::find().all(db.conn()).await.unwrap();
		let mut remaining_statuses = remaining
			.into_iter()
			.map(|job| job.status)
			.collect::<Vec<_>>();
		remaining_statuses.sort();
		let mut expected_statuses = vec![
			JobStatus::Queued.to_string(),
			JobStatus::Running.to_string(),
			JobStatus::Paused.to_string(),
		];
		expected_statuses.sort();

		assert_eq!(remaining_statuses, expected_statuses);

		let remaining_checkpoints = checkpoint::Entity::find().all(db.conn()).await.unwrap();
		let mut remaining_checkpoint_ids = remaining_checkpoints
			.into_iter()
			.map(|checkpoint| checkpoint.job_id)
			.collect::<Vec<_>>();
		remaining_checkpoint_ids.sort();
		expected_statuses.sort();

		assert_eq!(remaining_checkpoint_ids, expected_statuses);
	}
}
