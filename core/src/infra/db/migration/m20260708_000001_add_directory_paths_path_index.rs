//! Add an index on directory_paths(path).
//!
//! Path-based lookups against the directory cache — location-add entry reuse
//! (exact match), indexer parent resolution (IN probes), and descendant
//! prefix rewrites after moves (range scans) — all filter on `path`, which
//! previously had no index and forced full table scans.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
	async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
		manager
			.get_connection()
			.execute_unprepared(
				"CREATE INDEX IF NOT EXISTS idx_directory_paths_path \
				 ON directory_paths(path)",
			)
			.await?;

		Ok(())
	}

	async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
		manager
			.get_connection()
			.execute_unprepared("DROP INDEX IF EXISTS idx_directory_paths_path")
			.await?;

		Ok(())
	}
}
