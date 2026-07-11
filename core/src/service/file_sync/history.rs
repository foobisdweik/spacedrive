use crate::infra::db::entities::sync_generation;
use anyhow::Result;
use sea_orm::{
	prelude::*, sea_query::Expr, DatabaseConnection, FromQueryResult, QueryOrder, QuerySelect,
};
use std::sync::Arc;

/// Read-only queries over a conduit's generation history.
pub struct SyncHistory {
	db: Arc<DatabaseConnection>,
}

/// Aggregate statistics computed from a conduit's generation records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncHistoryStats {
	pub total_generations: u64,
	pub completed_generations: u64,
	pub verified_generations: u64,
	pub failed_generations: u64,
	pub files_copied: i64,
	pub files_deleted: i64,
	pub conflicts_resolved: i64,
	pub bytes_transferred: i64,
}

impl SyncHistory {
	pub fn new(db: Arc<DatabaseConnection>) -> Self {
		Self { db }
	}

	/// List generations for a conduit, newest first.
	pub async fn list_generations(
		&self,
		conduit_id: i32,
		limit: u64,
	) -> Result<Vec<sync_generation::Model>> {
		Ok(sync_generation::Entity::find()
			.filter(sync_generation::Column::ConduitId.eq(conduit_id))
			.order_by_desc(sync_generation::Column::Generation)
			.limit(limit)
			.all(&*self.db)
			.await?)
	}

	/// Fetch a specific generation by its sequence number.
	pub async fn get_generation(
		&self,
		conduit_id: i32,
		generation: i64,
	) -> Result<Option<sync_generation::Model>> {
		Ok(sync_generation::Entity::find()
			.filter(sync_generation::Column::ConduitId.eq(conduit_id))
			.filter(sync_generation::Column::Generation.eq(generation))
			.one(&*self.db)
			.await?)
	}

	/// Most recent generation regardless of completion status.
	pub async fn latest_generation(
		&self,
		conduit_id: i32,
	) -> Result<Option<sync_generation::Model>> {
		Ok(sync_generation::Entity::find()
			.filter(sync_generation::Column::ConduitId.eq(conduit_id))
			.order_by_desc(sync_generation::Column::Generation)
			.one(&*self.db)
			.await?)
	}

	/// Most recent generation that finished its copy/delete phases.
	pub async fn last_completed_generation(
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

	/// Most recent generation whose post-sync verification passed.
	pub async fn last_verified_generation(
		&self,
		conduit_id: i32,
	) -> Result<Option<sync_generation::Model>> {
		Ok(sync_generation::Entity::find()
			.filter(sync_generation::Column::ConduitId.eq(conduit_id))
			.filter(sync_generation::Column::VerificationStatus.eq("verified"))
			.order_by_desc(sync_generation::Column::Generation)
			.one(&*self.db)
			.await?)
	}

	/// Compute aggregate statistics across every generation of a conduit.
	///
	/// The aggregation runs entirely in the database (COUNT/SUM over a single
	/// scan) rather than loading every generation row into memory, so it stays
	/// O(1) in memory no matter how long a conduit's history grows.
	pub async fn stats(&self, conduit_id: i32) -> Result<SyncHistoryStats> {
		#[derive(Debug, FromQueryResult)]
		struct StatsRow {
			total_generations: Option<i64>,
			completed_generations: Option<i64>,
			verified_generations: Option<i64>,
			failed_generations: Option<i64>,
			files_copied: Option<i64>,
			files_deleted: Option<i64>,
			conflicts_resolved: Option<i64>,
			bytes_transferred: Option<i64>,
		}

		let row = sync_generation::Entity::find()
			.filter(sync_generation::Column::ConduitId.eq(conduit_id))
			.select_only()
			.column_as(sync_generation::Column::Id.count(), "total_generations")
			.column_as(
				Expr::cust("COUNT(CASE WHEN completed_at IS NOT NULL THEN 1 END)"),
				"completed_generations",
			)
			.column_as(
				Expr::cust("COUNT(CASE WHEN verification_status = 'verified' THEN 1 END)"),
				"verified_generations",
			)
			.column_as(
				Expr::cust("COUNT(CASE WHEN verification_status LIKE 'failed:%' THEN 1 END)"),
				"failed_generations",
			)
			.column_as(sync_generation::Column::FilesCopied.sum(), "files_copied")
			.column_as(sync_generation::Column::FilesDeleted.sum(), "files_deleted")
			.column_as(
				sync_generation::Column::ConflictsResolved.sum(),
				"conflicts_resolved",
			)
			.column_as(
				sync_generation::Column::BytesTransferred.sum(),
				"bytes_transferred",
			)
			.into_model::<StatsRow>()
			.one(&*self.db)
			.await?;

		// A bare aggregate query always yields one row, but guard against None
		// so an empty result set maps to zeroed stats rather than an error.
		let row = row.unwrap_or(StatsRow {
			total_generations: None,
			completed_generations: None,
			verified_generations: None,
			failed_generations: None,
			files_copied: None,
			files_deleted: None,
			conflicts_resolved: None,
			bytes_transferred: None,
		});

		Ok(SyncHistoryStats {
			total_generations: row.total_generations.unwrap_or(0) as u64,
			completed_generations: row.completed_generations.unwrap_or(0) as u64,
			verified_generations: row.verified_generations.unwrap_or(0) as u64,
			failed_generations: row.failed_generations.unwrap_or(0) as u64,
			files_copied: row.files_copied.unwrap_or(0),
			files_deleted: row.files_deleted.unwrap_or(0),
			conflicts_resolved: row.conflicts_resolved.unwrap_or(0),
			bytes_transferred: row.bytes_transferred.unwrap_or(0),
		})
	}
}
