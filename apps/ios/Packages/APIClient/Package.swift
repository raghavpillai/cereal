// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MeetingNotesAPIClient",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "APIClient", targets: ["APIClient"])],
    dependencies: [.package(path: "../Contracts")],
    targets: [
        .target(name: "APIClient", dependencies: ["Contracts"]),
        .testTarget(name: "APIClientTests", dependencies: ["APIClient", "Contracts"])
    ]
)
