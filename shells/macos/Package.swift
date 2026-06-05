// swift-tools-version: 6.0
import PackageDescription

// SafetyStrip — macOS shell.
//
// LINK-PATH ASSUMPTION
// --------------------
// The Rust core is built (out of tree) into the workspace target dir:
//
//     cargo build -p safetystrip-ffi --release   ->   target/release/libsafetystrip_ffi.a
//
// This package lives at `shells/macos`, so the staticlib sits two directories
// up at `../../target/release`. The linker flags below encode exactly that.
// If you build the core to a different location, override with:
//
//     swift build -Xlinker -L/abs/path/to/dir
//
// We link the *static* archive (`-lsafetystrip_ffi`) so the shipping binary has
// no runtime dependency on a SafetyStrip dylib. The Rust staticlib's own system
// dependencies (-lSystem -lc -lm) are provided automatically by the macOS SDK
// that SwiftPM already links, so no extra system libraries are required here.
let coreLinkerSettings: [LinkerSetting] = [
    .unsafeFlags([
        "-L../../target/release",
        "-lsafetystrip_ffi",
    ])
]

let package = Package(
    name: "SafetyStrip",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "SafetyStripApp", targets: ["SafetyStripApp"]),
        .library(name: "SafetyStripKit", targets: ["SafetyStripKit"]),
        .library(name: "SafetyStripCore", targets: ["SafetyStripCore"]),
    ],
    targets: [
        // C interop: surfaces the frozen `core-ffi/include/safetystrip.h` to Swift.
        .target(
            name: "CSafetyStrip"
        ),

        // Safe Swift wrapper over the C ABI. This is the only target that links
        // the Rust staticlib, so its dependents inherit the linker settings.
        .target(
            name: "SafetyStripCore",
            dependencies: ["CSafetyStrip"],
            linkerSettings: coreLinkerSettings
        ),

        // Headless shell logic (no UI): pasteboard, monitor, hotkey, settings,
        // controller. Unit-testable without a running app.
        .target(
            name: "SafetyStripKit",
            dependencies: ["SafetyStripCore"]
        ),

        // SwiftUI MenuBarExtra app. Wires a StripController to the menu UI.
        .executableTarget(
            name: "SafetyStripApp",
            dependencies: ["SafetyStripKit", "SafetyStripCore"]
        ),

        // Tests.
        .testTarget(
            name: "SafetyStripCoreTests",
            dependencies: ["SafetyStripCore"]
        ),
        .testTarget(
            name: "SafetyStripKitTests",
            dependencies: ["SafetyStripKit", "SafetyStripCore"]
        ),
    ]
)
