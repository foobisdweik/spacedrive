fn main() {
	swift_rs::SwiftLinker::new("11.0")
		.with_ios("11.0")
		.with_package("FileOpening", "./")
		.link();

	let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
	for configuration in ["Release", "Debug"] {
		println!(
			"cargo:rustc-link-search=native={}/swift-rs/FileOpening/out/Products/{}",
			out_dir, configuration
		);
	}
}
