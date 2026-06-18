#[cfg(target_os = "macos")]
use std::env;

fn main() {
	#[cfg(target_os = "macos")]
	{
		let deployment_target =
			env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| String::from("10.15"));

		swift_rs::SwiftLinker::new(deployment_target.as_str())
			.with_package("sd-desktop-macos", "./")
			.link();

		let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
		for configuration in ["Release", "Debug"] {
			println!(
				"cargo:rustc-link-search=native={}/swift-rs/sd-desktop-macos/out/Products/{}",
				out_dir, configuration
			);
		}
	}
}
