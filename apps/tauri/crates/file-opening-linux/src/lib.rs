use std::{
	io::Read,
	path::{Path, PathBuf},
};

use gio::prelude::*;

use file_opening::{FileOpener, OpenResult, OpenWithApp};

pub struct LinuxFileOpener;

/// Detect a file's content type from its name and leading magic bytes, so
/// extension-less files still resolve to the right handlers.
fn content_type_for(path: &Path) -> String {
	let mut head = Vec::with_capacity(4096);
	if let Ok(file) = std::fs::File::open(path) {
		let _ = file.take(4096).read_to_end(&mut head);
	}
	let (content_type, _uncertain) = gio::functions::content_type_guess(Some(path), &head);
	content_type.to_string()
}

fn file_uri(path: &Path) -> String {
	gio::File::for_path(path).uri().to_string()
}

fn app_info_for_id(app_id: &str) -> Option<gio::AppInfo> {
	gio::AppInfo::all()
		.into_iter()
		.find(|app| app.id().is_some_and(|id| id == app_id))
}

impl FileOpener for LinuxFileOpener {
	fn get_apps_for_file(&self, path: &Path) -> Result<Vec<OpenWithApp>, String> {
		if !path.exists() {
			return Ok(vec![]);
		}

		let content_type = content_type_for(path);
		let mut apps: Vec<OpenWithApp> = gio::AppInfo::recommended_for_type(&content_type)
			.into_iter()
			.filter_map(|app| {
				// The desktop entry ID (org.gnome.Evince.desktop) is the stable
				// identifier open_with_app resolves back through DesktopAppInfo.
				let id = app.id()?.to_string();
				Some(OpenWithApp {
					id,
					name: app.display_name().to_string(),
					icon: None,
				})
			})
			.collect();

		apps.sort_by(|a, b| a.name.cmp(&b.name));
		apps.dedup_by(|a, b| a.id == b.id);
		Ok(apps)
	}

	fn open_with_default(&self, path: &Path) -> Result<OpenResult, String> {
		if !path.exists() {
			return Ok(OpenResult::FileNotFound {
				path: path.to_string_lossy().to_string(),
			});
		}

		match gio::AppInfo::launch_default_for_uri(&file_uri(path), None::<&gio::AppLaunchContext>)
		{
			Ok(()) => Ok(OpenResult::Success),
			Err(e) => Ok(OpenResult::PlatformError {
				message: e.to_string(),
			}),
		}
	}

	fn open_with_app(&self, path: &Path, app_id: &str) -> Result<OpenResult, String> {
		if !path.exists() {
			return Ok(OpenResult::FileNotFound {
				path: path.to_string_lossy().to_string(),
			});
		}

		let Some(app) = app_info_for_id(app_id) else {
			return Ok(OpenResult::AppNotFound {
				app_id: app_id.to_string(),
			});
		};

		let files = [gio::File::for_path(path)];
		match app.launch(&files, None::<&gio::AppLaunchContext>) {
			Ok(()) => Ok(OpenResult::Success),
			Err(e) => Ok(OpenResult::PlatformError {
				message: e.to_string(),
			}),
		}
	}

	fn open_files_with_app(
		&self,
		paths: &[PathBuf],
		app_id: &str,
	) -> Result<Vec<OpenResult>, String> {
		let Some(app) = app_info_for_id(app_id) else {
			return Ok(paths
				.iter()
				.map(|_| OpenResult::AppNotFound {
					app_id: app_id.to_string(),
				})
				.collect());
		};

		// Launch all files in one call so the app opens a single instance
		// with every document, matching Files/Nautilus behavior.
		let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
		match app.launch(&files, None::<&gio::AppLaunchContext>) {
			Ok(()) => Ok(paths.iter().map(|_| OpenResult::Success).collect()),
			Err(e) => Ok(paths
				.iter()
				.map(|_| OpenResult::PlatformError {
					message: e.to_string(),
				})
				.collect()),
		}
	}
}
