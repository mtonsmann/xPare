# SafetyStrip

A **memory-safe, plain-text clipboard utility**. SafetyStrip cleans the text on
your clipboard — coerce rich text to plain, strip HTML and Markdown, normalize
whitespace, change case, and run line operations — and writes the result back **in
place**, without your clipboard content ever leaving the process.

Its whole reason to exist is trust: the clipboard holds passwords, tokens, PII, and
source, and the markup it carries is untrusted. So SafetyStrip is built so that

- **no clipboard content can leave the process** (no network anywhere, no
  persistence, no logging of content), and
- **the code that parses untrusted markup cannot be memory-unsafe** (the core is
  pure Rust with `#![forbid(unsafe_code)]`).

These are enforced mechanically by CI, not just promised. See [`SECURITY.md`](SECURITY.md).

## Two-component architecture

```
  native shell (Swift / …)  ──ss_transform(input, config_json)──▶  Rust core
  • owns the clipboard, hotkey, UI            C ABI                • pure, deterministic
  • reads rich text, extracts plain      (frozen, language-        • #![forbid(unsafe_code)]
  • writes the result back in place         neutral)              • no OS / IO / network
```

- A portable **Rust transformation core** (`core/`) — `String` in, `String` out,
  selected by a versioned JSON config. Pure, deterministic, never panics on input.
- Thin native **shells** (`shells/macos/` today; `windows/`/`linux/` reserved) that
  own all OS integration and call the core over a small, frozen **C ABI**
  (`core-ffi/`). Adding a platform means implementing the
  [shell contract](docs/guardrails/shell-contract.md) and linking the same core.

Full map: [`ARCHITECTURE.md`](ARCHITECTURE.md). Rationale: [`DESIGN.md`](DESIGN.md).

## Quickstart

Prerequisites: the pinned Rust stable toolchain (see `rust-toolchain.toml`).

### Build and test the core

```sh
cargo build --workspace
cargo test  --workspace
```

### Run the headless CLI harness

The `safetystrip` CLI (package `safetystrip-cli`) is a thin stdin → core → stdout
pipe with no clipboard or OS integration — handy for trying transforms:

```sh
# Strip HTML (note <script> bodies are dropped, tags removed):
echo '<b>hi</b><script>steal()</script>' | \
  cargo run -p safetystrip-cli -- transform \
    --config-json '{"version":1,"operations":[{"op":"strip_html"}]}'
# -> hi

# The canonical sanitization order, StripHtml then StripMarkdown:
echo '**bold** <i>x</i>' | \
  cargo run -p safetystrip-cli -- transform \
    --config-json '{"version":1,"operations":[{"op":"strip_html"},{"op":"strip_markdown"}]}'

# Ask the core what it can do:
cargo run -p safetystrip-cli -- capabilities
```

A config is `{"version":1,"operations":[ ... ]}` — an ordered list of operations
applied left to right. `transform` with no config flag is the identity pipeline.

### Run the full local gate

The single source of truth for "is it green" — the exact command CI runs:

```sh
cargo xtask ci
```

This runs `fmt --check`, `clippy -D warnings`, the test suite, and every structural
invariant check (no-unsafe-in-core, core dependency allowlist, no-network banlist,
frozen ABI header, minimal macOS entitlements). See [`CONTRIBUTING.md`](CONTRIBUTING.md).

### Fuzzing (optional, proves never-panics)

`fuzz/` is its own workspace and needs nightly + `cargo-fuzz`:

```sh
cargo +nightly fuzz run strip_html -- -max_total_time=60
```

Targets: `strip_html`, `strip_markdown`, `transform_pipeline`.

### Build the macOS shell

The shell links the FFI staticlib over the frozen C ABI:

```sh
cargo build -p safetystrip-ffi --release
swift build --package-path shells/macos
```

> `swift build` compiles the shell sources under Command-Line-Tools. Producing a
> signed, notarized `.app` needs full Xcode and is a documented step, not produced
> in the dev environment. See [the macOS posture](docs/guardrails/macos-posture.md).

## Repository layout

```
core/         pure transform core — #![forbid(unsafe_code)], no OS/IO/net
core-ffi/     thin C ABI shim (the only crate with `unsafe`) + the frozen header
cli/          headless harness over the core (binary: safetystrip)
xtask/        mechanical invariant enforcement — `cargo xtask <check>`
fuzz/         cargo-fuzz targets (separate workspace, nightly)
shells/macos  Swift menu-bar shell over the C ABI
shells/windows, shells/linux   reserved for future platforms
docs/         ARCHITECTURE / DESIGN / SECURITY, guardrails, exec plans
```

## Documentation

| Doc | What it covers |
|---|---|
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Repository map: crate/module responsibilities, the trust boundary, data flow, the enforced-invariants table |
| [`DESIGN.md`](DESIGN.md) | Every settled decision with rationale, the threat model, known limitations, and what's deferred until the project grows |
| [`SECURITY.md`](SECURITY.md) | Privacy/data-handling posture and how each property is enforced |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | The local gate, the per-change-class checklist, fuzzing |
| [`AGENTS.md`](AGENTS.md) | Short router: classify a change, then jump to the right guardrail |
| [`docs/guardrails/`](docs/guardrails/) | Focused, actionable rules per change class (transforms, memory safety, FFI/ABI, shells, macOS, privacy, dependencies) |

## License

Licensed under either of MIT or Apache-2.0 at your option.
