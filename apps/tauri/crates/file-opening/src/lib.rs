use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Represents an application that can open a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenWithApp {
	/// Platform-specific identifier:
	/// - macOS: bundle ID (com.apple.Preview)
	/// - Windows: application name
	/// - Linux: desktop entry ID (org.gnome.Evince.desktop)
	pub id: String,

	/// Human-readable display name
	pub name: String,

	/// Optional: app icon as base64-encoded PNG (for future use)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub icon: Option<String>,
}

/// Result of attempting to open a file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OpenResult {
	Success,
	FileNotFound { path: String },
	AppNotFound { app_id: String },
	PermissionDenied { path: String },
	PlatformError { message: String },
}

/// Trait for platform-specific file opening implementations
pub trait FileOpener: Send + Sync {
	/// Get list of applications that can open this file
	fn get_apps_for_file(&self, path: &Path) -> Result<Vec<OpenWithApp>, String>;

	/// Get list of apps that can open all provided files (intersection)
	fn get_apps_for_files(&self, paths: &[PathBuf]) -> Result<Vec<OpenWithApp>, String> {
		if paths.is_empty() {
			return Ok(vec![]);
		}

		// Get apps for first file
		let mut common_apps = self
			.get_apps_for_file(&paths[0])?
			.into_iter()
			.map(|app| (app.id.clone(), app))
			.collect::<HashMap<_, _>>();

		// Intersect with remaining files
		for path in &paths[1..] {
			let apps = self
				.get_apps_for_file(path)?
				.into_iter()
				.map(|app| app.id)
				.collect::<HashSet<_>>();

			common_apps.retain(|id, _| apps.contains(id));
		}

		let mut result: Vec<_> = common_apps.into_values().collect();
		result.sort_by(|a, b| a.name.cmp(&b.name));
		Ok(result)
	}

	/// Open file with system default application
	fn open_with_default(&self, path: &Path) -> Result<OpenResult, String>;

	/// Open file with specific application
	fn open_with_app(&self, path: &Path, app_id: &str) -> Result<OpenResult, String>;

	/// Open multiple files with specific application
	fn open_files_with_app(
		&self,
		paths: &[PathBuf],
		app_id: &str,
	) -> Result<Vec<OpenResult>, String> {
		paths
			.iter()
			.map(|path| self.open_with_app(path, app_id))
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct MockOpener {
		apps_by_path: HashMap<PathBuf, Vec<OpenWithApp>>,
	}

	impl FileOpener for MockOpener {
		fn get_apps_for_file(&self, path: &Path) -> Result<Vec<OpenWithApp>, String> {
			Ok(self.apps_by_path.get(path).cloned().unwrap_or_default())
		}

		fn open_with_default(&self, _path: &Path) -> Result<OpenResult, String> {
			unreachable!("not used by intersection tests")
		}

		fn open_with_app(&self, _path: &Path, _app_id: &str) -> Result<OpenResult, String> {
			unreachable!("not used by intersection tests")
		}
	}

	fn app(id: &str, name: &str) -> OpenWithApp {
		OpenWithApp {
			id: id.to_string(),
			name: name.to_string(),
			icon: None,
		}
	}

	#[test]
	fn get_apps_for_files_intersects_and_sorts_by_name() {
		let first = PathBuf::from("first.txt");
		let second = PathBuf::from("second.jpg");
		let opener = MockOpener {
			apps_by_path: HashMap::from([
				(
					first.clone(),
					vec![app("editor", "Text Editor"), app("viewer", "Image Viewer")],
				),
				(
					second.clone(),
					vec![app("viewer", "Image Viewer"), app("other", "Other")],
				),
			]),
		};

		let apps = opener.get_apps_for_files(&[first, second]).unwrap();

		assert_eq!(apps.len(), 1);
		assert_eq!(apps[0].id, "viewer");
		assert_eq!(apps[0].name, "Image Viewer");
	}

	#[test]
	fn get_apps_for_files_returns_empty_for_no_paths() {
		let opener = MockOpener {
			apps_by_path: HashMap::new(),
		};

		assert!(opener.get_apps_for_files(&[]).unwrap().is_empty());
	}
}
