#[cfg(target_os = "macos")]
use std::process::Command;

fn main() {
	// Compile .icon to Assets.car on macOS
	#[cfg(target_os = "macos")]
	{
		let project_root = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
		let icon_source = format!("{}/../Spacedrive.icon", project_root);
		let gen_dir = format!("{}/gen", project_root);

		// Create gen directory
		std::fs::create_dir_all(&gen_dir).expect("Failed to create gen directory");

		// Check if .icon file exists
		if std::path::Path::new(&icon_source).exists() {
			println!("cargo:rerun-if-changed={}", icon_source);

			// Run actool to compile .icon to Assets.car
			let output = Command::new("xcrun")
				.args([
					"actool",
					&icon_source,
					"--compile",
					&gen_dir,
					"--output-format",
					"human-readable-text",
					"--notices",
					"--warnings",
					"--errors",
					"--output-partial-info-plist",
					&format!("{}/partial.plist", gen_dir),
					"--app-icon",
					"Spacedrive",
					"--include-all-app-icons",
					"--enable-on-demand-resources",
					"NO",
					"--development-region",
					"en",
					"--target-device",
					"mac",
					"--minimum-deployment-target",
					"11.0",
					"--platform",
					"macosx",
				])
				.output()
				.expect("Failed to execute actool");

			if !output.status.success() {
				eprintln!("actool failed: {}", String::from_utf8_lossy(&output.stderr));
			} else {
				println!("Successfully compiled Spacedrive.icon to Assets.car");
			}
		} else {
			println!("cargo:warning=Spacedrive.icon not found at {}", icon_source);
		}
	}

	// Create target-suffixed daemon binary for Tauri bundler
	// Tauri's externalBin expects binaries with target triple suffix
	let target_triple = std::env::var("TARGET").expect("TARGET not set");

	// Expose target triple to runtime code for daemon binary resolution
	println!("cargo:rustc-env=SD_TARGET_TRIPLE={}", target_triple);
	let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
	let workspace_dir = std::env::var("CARGO_WORKSPACE_DIR")
		.or_else(|_| std::env::var("CARGO_MANIFEST_DIR").map(|d| format!("{}/../../..", d)))
		.expect("Could not find workspace directory");

	let exe_ext = if target_triple.contains("windows") {
		".exe"
	} else {
		""
	};

	let source_candidates = [
		format!(
			"{}/target/{}/{}/sd-daemon{}",
			workspace_dir, target_triple, profile, exe_ext
		),
		format!("{}/target/{}/sd-daemon{}", workspace_dir, profile, exe_ext),
		format!(
			"{}/target/{}/release/sd-daemon{}",
			workspace_dir, target_triple, exe_ext
		),
		format!("{}/target/release/sd-daemon{}", workspace_dir, exe_ext),
		format!(
			"{}/target/{}/debug/sd-daemon{}",
			workspace_dir, target_triple, exe_ext
		),
		format!("{}/target/debug/sd-daemon{}", workspace_dir, exe_ext),
	];

	if let Some(daemon_source) = source_candidates
		.iter()
		.map(std::path::Path::new)
		.find(|path| path.exists() && path.metadata().is_ok_and(|meta| meta.len() > 0))
	{
		for target_profile in [profile.as_str(), "release"] {
			let daemon_target = format!(
				"{}/target/{}/sd-daemon-{}{}",
				workspace_dir, target_profile, target_triple, exe_ext
			);
			let daemon_target_path = std::path::Path::new(&daemon_target);

			if let Some(parent) = daemon_target_path.parent() {
				std::fs::create_dir_all(parent).expect("Failed to create Tauri sidecar directory");
			}

			let _ = std::fs::remove_file(daemon_target_path);
			std::fs::copy(daemon_source, daemon_target_path)
				.expect("Failed to copy daemon sidecar");
		}
	} else {
		eprintln!(
			"cargo:warning=sd-daemon sidecar was not found. Run `bun run build:daemon` before bundling."
		);
	}

	tauri_build::build()
}
