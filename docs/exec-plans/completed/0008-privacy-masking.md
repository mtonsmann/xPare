# Exec Plan 0008 — Privacy masking

Status: **completed** · Started: 2026-06-06 · Completed: 2026-06-06

Based on `main` at `6461085` after `git fetch origin main --prune` on
2026-06-06.

## Goal

Add an optional **privacy masking** rewrite that replaces selected clipboard
identifiers with fixed placeholders before paste. First scope:

- email-like tokens,
- standalone IPv4 addresses,
- standalone IPv6 addresses.

The feature complements `Defang`: defang keeps indicators readable but inert;
masking hides their values. It must stay local, deterministic, pure, and
memory-safe, and it must not claim to be complete anonymization or DLP.

## Change class

Core transform + Swift shell UX + docs.

Compatibility/posture summary:

- **No C ABI change.** This is a new config operation crossing the existing JSON
  config channel.
- **No `CONFIG_VERSION` bump.** The current schema remains version 2; this is an
  additive operation variant.
- **No privacy posture change.** The feature changes transform output when enabled;
  it does not add network, persistence, logging, entitlements, or a new content
  data path.
- **Supported transforms change.** Update capabilities and docs.

## Decisions

### D-1 — Add one parameterized rewrite op

Add a single operation:

```json
{
  "op": "mask_identifiers",
  "emails": true,
  "ipv4": true,
  "ipv6": true
}
```

Rust shape:

```rust
Operation::MaskIdentifiers {
    emails: bool,
    ipv4: bool,
    ipv6: bool,
}
```

Each boolean defaults to `false` when absent, so callers may send
`{"op":"mask_identifiers","emails":true}`. The shell must not persist a
`mask_identifiers` op with every target off; that no-target form is treated as a
wire-level no-op, not a normal UI state.

Why one op instead of three: email/IP masking uses the same token walk, so one op
keeps the common case to one pass and leaves the shell free to show separate target
toggles.

### D-2 — Use fixed placeholders, not partial masks or pseudonyms

Outputs:

| Target | Placeholder |
|---|---|
| Email | `[email]` |
| IPv4 | `[ipv4]` |
| IPv6 | `[ipv6]` |

No partial preservation such as `m***@example.com`, no hashing, no random values,
no per-session maps, and no counters. Fixed placeholders are deterministic,
idempotent, leak the least, and require no persistence.

### D-3 — Reuse one indicator heuristic source

Move shared token/indicator helpers into `core/src/ops/indicators.rs`, so
`extract_*`, `defang`, `clean_urls`, and `mask_identifiers` do not drift.

Shared helpers:

- `trim_token_punct`,
- `is_email`,
- `is_url`,
- `is_ipv4`,
- `is_ipv6`.

Keep the helpers `pub(crate)`, pure, dependency-free, and documented as
heuristics, not validators.

### D-4 — V1 masking is token-level and honest about limits

Masking follows xPare's existing token model: split on whitespace, trim the
same fixed surrounding punctuation set, classify the trimmed token core, and
re-emit surrounding punctuation unchanged.

That means v1 masks ordinary clipboard/log shapes such as:

- `user@example.com`,
- `<user@example.com>`,
- `192.168.0.1`,
- `2001:db8::1`.

It deliberately does **not** promise comprehensive PII detection. Do not claim to
mask names, phone numbers, postal addresses, secrets, all URLs, or arbitrary emails
embedded inside larger non-whitespace strings. URL-host IP masking is deferred
unless we explicitly widen the URL token contract in a later decision.

### D-5 — Canonical order masks before defang and reductions

Canonical ordering should run `MaskIdentifiers` after rich/plain cleanup and URL
tracker cleanup, but before `Defang`, `Refang`, and extraction reductions.

Proposed rank:

| Rank | Op |
|--:|---|
| 1 | `StripHtml` |
| 2 | `StripMarkdown` |
| 3 | `SplitOn` |
| 4 | `UnwrapLines` |
| 5 | `CollapseWhitespace` |
| 6 | `TrimTrailingWhitespace` |
| 7 | `CleanUrls` |
| 8 | `MaskIdentifiers` |
| 9 | `Defang` |
| 10 | `Refang` |
| 11 | `ExtractEmails` |
| 12 | `ExtractUrls` |
| 13+ | existing line/case/decorating ranks shifted as needed |

If masking and extraction appear together in a canonical CLI pipeline, extraction
will see masked text and may yield an empty result. That is the privacy-preserving
default. Power users can use `ordering:"as_given"` if they deliberately want a
different order.

## Non-goals

- No ABI change or new FFI entry point.
- No new dependency or regex engine.
- No machine-learning or broad PII detector.
- No reversible masking, hashing, salting, or persistent pseudonym map.
- No shell-side transform logic.
- No new entitlement, network capability, content persistence, or content logging.
- No guarantee against the original OS clipboard value, other local pasteboard
  readers, or same-user race conditions.

## Workstreams

1. Add shared indicator helpers and keep existing extractor/defang/URL behavior
   unchanged.
2. Add the core `mask_identifiers` op, config variant, canonical rank, pipeline
   dispatch, capabilities entry, and focused tests.
3. Add Swift `TransformConfig` mirror support and macOS menu/settings wiring.
4. Update architecture, design, security, guardrails, README, fuzz, and perf guards.

## Verification Plan

Minimum checks before PR:

```sh
cargo fmt --all --check
cargo clippy -p xpare-core --all-targets -- -D warnings
cargo test -p xpare-core
cargo test -p xpare-core --test perf_guard
cargo xtask check-abi
cargo xtask check-no-network
cargo xtask check-no-content-logging
cargo xtask check-pipeline-zeroization
cargo build -p xpare-ffi --release
swift build --package-path shells/macos
```

Fuzz smoke for the changed transform path:

```sh
cargo +nightly fuzz run mask_identifiers -- -max_total_time=60
cargo +nightly fuzz run transform_pipeline -- -max_total_time=60
```

Before merge, run the full gate:

```sh
cargo xtask ci
```

## Verification Result

Completed on 2026-06-06:

- `cargo test -p xpare-core`
- `cargo clippy -p xpare-core --all-targets -- -D warnings`
- `cargo run -p xtask -- check-abi`
- `cargo run -p xtask -- check-no-network`
- `cargo run -p xtask -- check-no-content-logging`
- `cargo run -p xtask -- check-pipeline-zeroization`
- `cargo run -p xtask -- check-unsafe-forbid`
- `cargo run -p xtask -- check-core-deps`
- `cargo build -p xpare-ffi --release`
- `./shells/macos/build.sh test`
- `cargo test --workspace`
- `make ci`

`cargo-fuzz` was not installed locally, so the fuzz smoke commands were not run.
The new fuzz targets were added for CI/developer environments that have
`cargo-fuzz` available.

## PR callouts

The PR must explicitly state:

- change class: core transform + shell UX + docs,
- C ABI unchanged,
- `CONFIG_VERSION` remains 2,
- privacy posture unchanged: no network, persistence, logging, or entitlement,
- masking is heuristic token-level masking, not comprehensive anonymization,
- regression protection added: golden tests, properties, corpus, fuzz target, and
  perf guard.
