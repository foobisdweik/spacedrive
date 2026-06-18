// swift-tools-version: 5.5
import PackageDescription

let package = Package(
    name: "SwiftRs",
    platforms: [
        .macOS(.v10_13),
        .iOS(.v11),
    ],
    products: [
        .library(
            name: "SwiftRs",
            targets: ["SwiftRs"]
        ),
    ],
    targets: [
        .target(
            name: "SwiftRs",
            path: "src-swift"
        ),
    ]
)
