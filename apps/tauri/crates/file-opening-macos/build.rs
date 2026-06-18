fn main() {
	swift_rs::SwiftLinker::new("11.0")
		.with_ios("11.0")
		.with_package("FileOpening", "./")
		.link();

	let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
	let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
	let configuration = if profile == "release" {
		"Release"
	} else {
		"Debug"
	};
	println!(
		"cargo:rustc-link-search=native={}/swift-rs/FileOpening/out/Products/{}",
		out_dir, configuration
	);
}
