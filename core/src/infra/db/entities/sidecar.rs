use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "sidecar")]
pub struct Model {
	#[sea_orm(primary_key)]
	pub id: i32,

	pub uuid: Uuid,

	pub content_uuid: Uuid,

	pub kind: String,

	pub variant: String,

	pub format: String,

	pub rel_path: String,

	/// For reference sidecars, the entry ID of the original file
	/// This allows sidecars to reference existing entries without moving them
	pub source_entry_id: Option<i32>,

	pub size: i64,

	pub checksum: Option<String>,

	pub status: String,

	pub source: Option<String>,

	pub version: i32,

	pub created_at: DateTime<Utc>,

	pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
	#[sea_orm(
		belongs_to = "super::content_identity::Entity",
		from = "Column::ContentUuid",
		to = "super::content_identity::Column::Uuid"
	)]
	ContentIdentity,

	#[sea_orm(
		belongs_to = "super::entry::Entity",
		from = "Column::SourceEntryId",
		to = "super::entry::Column::Id"
	)]
	SourceEntry,
}

impl Related<super::content_identity::Entity> for Entity {
	fn to() -> RelationDef {
		Relation::ContentIdentity.def()
	}
}

impl Related<super::entry::Entity> for Entity {
	fn to() -> RelationDef {
		Relation::SourceEntry.def()
	}
}

impl ActiveModelBehavior for ActiveModel {}

pub async fn affected_entry_uuids_for_change(
	entry: &crate::infra::sync::SharedChangeEntry,
	db: &DatabaseConnection,
) -> Result<Vec<Uuid>, DbErr> {
	use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

	if entry.model_type != "sidecar"
		|| !matches!(entry.change_type, crate::infra::sync::ChangeType::Delete)
	{
		return Ok(Vec::new());
	}

	let content_uuid = entry
		.data
		.get("content_uuid")
		.cloned()
		.ok_or_else(|| DbErr::Custom("Sidecar deletion is missing content_uuid".to_string()))
		.and_then(|value| {
			serde_json::from_value::<Uuid>(value)
				.map_err(|error| DbErr::Custom(format!("Invalid sidecar content UUID: {error}")))
		})?;
	let Some(content) = super::content_identity::Entity::find()
		.filter(super::content_identity::Column::Uuid.eq(content_uuid))
		.one(db)
		.await?
	else {
		return Ok(Vec::new());
	};

	Ok(super::entry::Entity::find()
		.filter(super::entry::Column::ContentId.eq(content.id))
		.all(db)
		.await?
		.into_iter()
		.filter_map(|entry| entry.uuid)
		.collect())
}

// Sidecars are SHARED resources (content-scoped, not device-owned).
// All devices should know what sidecars exist globally via library sync.
// Actual file availability is tracked separately in sidecar_availability (local only).
impl crate::infra::sync::Syncable for Model {
	const SYNC_MODEL: &'static str = "sidecar";

	fn sync_id(&self) -> Uuid {
		self.uuid
	}

	fn version(&self) -> i64 {
		self.version as i64
	}

	fn exclude_fields() -> Option<&'static [&'static str]> {
		Some(&[
			"id",              // Local database ID
			"source_entry_id", // Local entry reference
		])
	}

	fn sync_depends_on() -> &'static [&'static str] {
		&["content_identity"] // Sidecars depend on content existing first
	}

	fn foreign_key_mappings() -> Vec<crate::infra::sync::FKMapping> {
		vec![
			// Map content_uuid FK to content_identities table
			crate::infra::sync::FKMapping::new("content_uuid", "content_identities"),
		]
	}

	async fn apply_shared_change(
		entry: crate::infra::sync::SharedChangeEntry,
		db: &DatabaseConnection,
	) -> Result<(), sea_orm::DbErr> {
		use crate::infra::sync::ChangeType;
		use sea_orm::{
			sea_query::OnConflict, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, EntityTrait,
			QueryFilter,
		};

		#[derive(Deserialize)]
		struct SyncedSidecar {
			uuid: Uuid,
			content_uuid: Uuid,
			kind: String,
			variant: String,
			format: String,
			rel_path: String,
			size: i64,
			checksum: Option<String>,
			status: String,
			source: Option<String>,
			version: i32,
			created_at: DateTime<Utc>,
			updated_at: DateTime<Utc>,
		}

		match entry.change_type {
			ChangeType::Insert | ChangeType::Update => {
				let sidecar: SyncedSidecar =
					serde_json::from_value(entry.data).map_err(|error| {
						sea_orm::DbErr::Custom(format!("Invalid sidecar sync data: {error}"))
					})?;
				let active = ActiveModel {
					id: NotSet,
					uuid: Set(sidecar.uuid),
					content_uuid: Set(sidecar.content_uuid),
					kind: Set(sidecar.kind),
					variant: Set(sidecar.variant),
					format: Set(sidecar.format),
					rel_path: Set(sidecar.rel_path),
					source_entry_id: Set(None),
					size: Set(sidecar.size),
					checksum: Set(sidecar.checksum),
					status: Set(sidecar.status),
					source: Set(sidecar.source),
					version: Set(sidecar.version),
					created_at: Set(sidecar.created_at),
					updated_at: Set(sidecar.updated_at),
				};

				Entity::insert(active)
					.on_conflict(
						OnConflict::columns([Column::ContentUuid, Column::Kind, Column::Variant])
							.update_columns([
								Column::Format,
								Column::RelPath,
								Column::Size,
								Column::Checksum,
								Column::Status,
								Column::Source,
								Column::Version,
								Column::UpdatedAt,
							])
							.to_owned(),
					)
					.exec(db)
					.await?;
			}
			ChangeType::Delete => {
				let synced: SyncedSidecar =
					serde_json::from_value(entry.data).map_err(|error| {
						sea_orm::DbErr::Custom(format!("Invalid sidecar sync data: {error}"))
					})?;
				if let Some(sidecar) = Entity::find()
					.filter(Column::ContentUuid.eq(synced.content_uuid))
					.filter(Column::Kind.eq(&synced.kind))
					.filter(Column::Variant.eq(&synced.variant))
					.one(db)
					.await?
				{
					crate::service::sidecar_manager::remove_synced_managed_sidecar(db, &sidecar)
						.await?;
					super::sidecar_availability::Entity::delete_many()
						.filter(
							super::sidecar_availability::Column::ContentUuid
								.eq(sidecar.content_uuid),
						)
						.filter(super::sidecar_availability::Column::Kind.eq(sidecar.kind))
						.filter(super::sidecar_availability::Column::Variant.eq(sidecar.variant))
						.exec(db)
						.await?;
				} else {
					crate::service::sidecar_manager::mark_synced_sidecar_deleted_from_db(
						db,
						synced.content_uuid,
						&synced.kind,
						&synced.variant,
					)
					.await?;
				}

				Entity::delete_many()
					.filter(Column::ContentUuid.eq(synced.content_uuid))
					.filter(Column::Kind.eq(synced.kind))
					.filter(Column::Variant.eq(synced.variant))
					.exec(db)
					.await?;
			}
		}

		Ok(())
	}

	async fn query_for_sync(
		_device_id: Option<Uuid>,
		since: Option<chrono::DateTime<chrono::Utc>>,
		cursor: Option<(chrono::DateTime<chrono::Utc>, Uuid)>,
		batch_size: usize,
		db: &DatabaseConnection,
	) -> Result<Vec<(Uuid, serde_json::Value, chrono::DateTime<chrono::Utc>)>, sea_orm::DbErr> {
		use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

		let mut query = Entity::find();

		if let Some(since_time) = since {
			query = query.filter(Column::UpdatedAt.gte(since_time));
		}

		// Cursor-based pagination
		if let Some((cursor_ts, cursor_uuid)) = cursor {
			query = query.filter(
				Condition::any().add(Column::UpdatedAt.gt(cursor_ts)).add(
					Condition::all()
						.add(Column::UpdatedAt.eq(cursor_ts))
						.add(Column::Uuid.gt(cursor_uuid)),
				),
			);
		}

		let results = query
			.order_by_asc(Column::UpdatedAt)
			.order_by_asc(Column::Uuid)
			.limit(batch_size as u64)
			.all(db)
			.await?;

		// Convert to sync format
		let mut sync_data = Vec::new();
		for model in results {
			let json = serde_json::to_value(&model)
				.map_err(|e| DbErr::Custom(format!("Failed to serialize sidecar: {}", e)))?;
			sync_data.push((model.uuid, json, model.updated_at));
		}

		Ok(sync_data)
	}
}

// Register with sync system via inventory
crate::register_syncable_shared!(Model, "sidecar", "sidecars");
