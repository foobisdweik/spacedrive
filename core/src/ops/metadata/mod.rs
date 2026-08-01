//! Metadata operations module
//!
//! This module contains business logic for managing user metadata,
//! including semantic tagging integration.

pub mod manager;
pub mod set_favorite;

pub use manager::UserMetadataManager;
pub use set_favorite::*;
