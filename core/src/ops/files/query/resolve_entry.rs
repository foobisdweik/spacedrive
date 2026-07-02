//! Resolve frontend entry UUIDs to local database entry IDs.

use std::sync::Arc;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::{
	context::CoreContext,
	infra::{
		api::SessionContext,
		db::entities::entry,
		query::{LibraryQuery, QueryError, QueryResult},
	},
};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolveEntryInput {
	pub entry_uuid: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolveEntryOutput {
	pub entry_uuid: Uuid,
	pub entry_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResolveEntryQuery {
	input: ResolveEntryInput,
}

impl LibraryQuery for ResolveEntryQuery {
	type Input = ResolveEntryInput;
	type Output = ResolveEntryOutput;

	fn from_input(input: Self::Input) -> QueryResult<Self> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		context: Arc<CoreContext>,
		session: SessionContext,
	) -> QueryResult<Self::Output> {
		let library_id = session
			.current_library_id
			.ok_or_else(|| QueryError::Internal("No library in session".to_string()))?;
		let library = context
			.libraries()
			.await
			.get_library(library_id)
			.await
			.ok_or_else(|| QueryError::LibraryNotFound(library_id))?;

		let entry = entry::Entity::find()
			.filter(entry::Column::Uuid.eq(self.input.entry_uuid))
			.one(library.db().conn())
			.await?
			.ok_or_else(|| QueryError::Internal("Entry not found".to_string()))?;

		Ok(ResolveEntryOutput {
			entry_uuid: self.input.entry_uuid,
			entry_id: entry.id,
		})
	}
}

crate::register_library_query!(ResolveEntryQuery, "files.entry.resolve");
