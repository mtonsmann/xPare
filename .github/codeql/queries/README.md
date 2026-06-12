# xPare custom CodeQL queries

These packs encode project-specific policy that is useful as GitHub code-scanning
review signal and awkward to express through built-in CodeQL alone.

- `rust/` flags shipped Rust drift toward process/network capability and keeps
  the pure core/FFI boundary free of filesystem/path APIs. It covers resolved
  call targets plus `std::fs`/`std::path` imports and type references so those
  capabilities cannot enter public signatures or fields without a CodeQL alert.
  Grouped imports such as `use std::{path::PathBuf, fs};` are resolved through
  their parent use-tree path, and `std::net` imports/types are covered across
  shipped Rust surfaces before a call site appears.
  The CLI remains allowed to read its config file.
- `python/` keeps the macOS icon helper stdlib-only and capability-light by
  rejecting imports or calls that add network, process, concurrency,
  persistence, native-code, or dynamic-execution capability.

The required local gate is still `cargo xtask ci`. `cargo xtask
check-codeql-workflow-posture` keeps these packs wired into the additive CodeQL
workflow.
