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
// The static archive is passed as an EXPLICIT linker input, never via
// `-L… -lxpare_ffi`: with `-l`, macOS ld prefers a libxpare_ffi.dylib over the
// .a in the same directory, and cargo used to emit both — which is how the
// rc.2 preview shipped a binary whose dylib install_name pointed into the
// build machine's target/ tree. An explicit path cannot be ambushed by stale
// dylibs. The staticlib's own system dependencies (-lSystem -lc -lm) come from
// the macOS SDK that SwiftPM already links.
let coreLinkerSettings: [LinkerSetting] = [
    .unsafeFlags([
        "../../target/release/libxpare_ffi.a"
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
