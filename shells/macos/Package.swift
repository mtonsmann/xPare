// swift-tools-version: 6.0
import PackageDescription

// xPare — macOS shell.
//
// LINK-PATH ASSUMPTION
// --------------------
// The Rust core is built (out of tree) into the workspace target dir:
//
//     cargo build -p xpare-ffi --release   ->   target/release/libxpare_ffi.a
//
// This package lives at `shells/macos`, so the staticlib sits two directories
// up at `../../target/release`. The linker flags below encode exactly that.
// If you build the core to a different location, override with:
//
//     swift build -Xlinker -L/abs/path/to/dir
//
// We link the *static* archive (`-lxpare_ffi`) so the shipping binary has
// no runtime dependency on a xPare dylib. The Rust staticlib's own system
// dependencies (-lSystem -lc -lm) are provided automatically by the macOS SDK
// that SwiftPM already links, so no extra system libraries are required here.
let coreLinkerSettings: [LinkerSetting] = [
    .unsafeFlags([
        "-L../../target/release",
        "-lxpare_ffi",
    ])
]

let package = Package(
    name: "xPare",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "XPareApp", targets: ["XPareApp"]),
        .library(name: "XPareKit", targets: ["XPareKit"]),
        .library(name: "XPareCore", targets: ["XPareCore"]),
    ],
    targets: [
        // C interop: surfaces the frozen `core-ffi/include/xpare.h` to Swift.
        .target(
            name: "CXPare"
        ),

        // Safe Swift wrapper over the C ABI. This is the only target that links
        // the Rust staticlib, so its dependents inherit the linker settings.
        .target(
            name: "XPareCore",
            dependencies: ["CXPare"],
            linkerSettings: coreLinkerSettings
        ),

        // Headless shell logic (no UI): pasteboard, monitor, hotkey, settings,
        // controller. Unit-testable without a running app.
        .target(
            name: "XPareKit",
            dependencies: ["XPareCore"]
        ),

        // SwiftUI MenuBarExtra app. Wires a StripController to the menu UI.
        .executableTarget(
            name: "XPareApp",
            dependencies: ["XPareKit", "XPareCore"]
        ),

        // Tests.
        .testTarget(
            name: "XPareCoreTests",
            dependencies: ["XPareCore"]
        ),
        .testTarget(
            name: "XPareKitTests",
            dependencies: ["XPareKit", "XPareCore"]
        ),
    ]
)
