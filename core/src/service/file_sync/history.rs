use crate::infra::db::entities::sync_generation;
use anyhow::Result;
use sea_orm::{prelude::*, DatabaseConnection, QueryOrder, QuerySelect};
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
	pub async fn stats(&self, conduit_id: i32) -> Result<SyncHistoryStats> {
		let generations = sync_generation::Entity::find()
			.filter(sync_generation::Column::ConduitId.eq(conduit_id))
			.all(&*self.db)
			.await?;

		let mut stats = SyncHistoryStats::default();
		for generation in &generations {
			stats.total_generations += 1;
			if generation.completed_at.is_some() {
				stats.completed_generations += 1;
			}
			if generation.verification_status == "verified" {
				stats.verified_generations += 1;
			}
			if generation.verification_status.starts_with("failed:") {
				stats.failed_generations += 1;
			}
			stats.files_copied += generation.files_copied as i64;
			stats.files_deleted += generation.files_deleted as i64;
			stats.conflicts_resolved += generation.conflicts_resolved as i64;
			stats.bytes_transferred += generation.bytes_transferred;
		}

		Ok(stats)
	}
}
