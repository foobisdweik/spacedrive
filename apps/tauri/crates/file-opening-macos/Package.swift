// swift-tools-version: 5.5
import PackageDescription

let package = Package(
    name: "FileOpening",
    platforms: [
        .macOS(.v11),
        .iOS(.v14)
    ],
    products: [
        .library(
            name: "FileOpening",
            type: .static,
            targets: ["FileOpening"]
        )
    ],
    dependencies: [
        .package(path: "../swift-rs"),
    ],
    targets: [
        .target(
            name: "FileOpening",
            dependencies: [
                .product(name: "SwiftRs", package: "swift-rs")
            ],
            path: "src-swift"
        )
    ]
)
