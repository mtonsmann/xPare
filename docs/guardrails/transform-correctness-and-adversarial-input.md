# Guardrail: transform correctness & adversarial input

**When to consult:** you are changing transform logic — the HTML or Markdown
stripper, whitespace/case/line ops, the extractors, or the pipeline
(`core/src/ops/*.rs`, `core/src/pipeline.rs`, `core/src/config.rs`). Pair this with
[memory-safety](memory-safety.md).

The core is the **untrusted-input path**: it is fed arbitrary, attacker-influenced
clipboard markup. Correctness here means "right output" *and* "never panics, never
hangs, stays deterministic, neutralizes active content."

## The rules

1. **Never panic on any input.** No `unwrap`/`expect`/`panic!`/`unreachable!`/`[]`
   indexing by an input-derived value, no slicing a `&str` at a byte offset that
   could fall off a UTF-8 boundary. Iterate by `char` / `char_indices` (the
   strippers do) and use checked lookups (`str::get`, `.position()`).
2. **Stay linear-time with bounded lookahead.** A clipboard paste can be large and
   hostile; an O(n²) scan or unbounded backtracking is a denial-of-service. The HTML
   stripper makes one forward pass; entity and close-tag lookahead are explicitly
   bounded (`MAX_NUMERIC_DIGITS`, `MAX_ENTITY_NAME_LEN`, a single forward `find`).
3. **Be deterministic.** Same `(input, config)` ⇒ same output, with no dependence on
   environment, time, locale, or hash-set iteration order. (`dedupe_lines` uses a
   `HashSet` for membership only and emits in original order — copy that pattern.)
4. **Preserve the documented contract.** Each op's exact, frozen rules live in the
   doc comment on its function — that is the source of truth. If you change behavior,
   change the doc comment in the same diff and update the tests. Do not silently
   drift `strip_html`'s block set, the entity table, the line model, the unwrap rule,
   or the title/sentence-case rules.
5. **Honor the sanitization boundary.** `strip_html` is the security workhorse that
   neutralizes `<script>`/`<style>` and removes tags; the shell runs it on the
   clipboard's HTML representation. `strip_markdown` removes Markdown formatting and
   delegates *embedded* HTML to `strip_html` best-effort, but is **not** itself the
   script-neutralizing boundary. The canonical order is **`StripHtml` →
   `StripMarkdown`**. Do not weaken `strip_html`'s raw-text handling, and do not
   reframe `strip_markdown` as the sanitizer.
6. **Adding a transform is data, not API.** A new operation is a new `Operation`
   enum variant in `config.rs`, a match arm in `pipeline.rs`, an entry in
   `CAPABILITIES_JSON` (`core/src/lib.rs`), and its own pure function in `ops/`. It
   must **not** touch the C ABI (see [ffi-boundary-and-abi-stability](ffi-boundary-and-abi-stability.md))
   and must keep the core's dependency tree on the allowlist
   (see [dependency-posture](dependency-posture.md)).
7. **Keep the core pure.** No OS, IO, network, logging, or global mutable state. A
   stray `println!`/`dbg!` is a compile error by design — leave it that way.

## The documented op rules (where they live)

| Area | File | Notes you must preserve |
|---|---|---|
| HTML | `core/src/ops/html.rs` | comment/declaration/PI dropping; quoted-attr `>`; stray `<`/`>` emitted literally; `<script>`/`<style>` raw-text dropped (case-insensitive close, unterminated → drop to end); curated block set; `<br>`/`<hr>` newline; ≤ one blank line; numeric entities (surrogate/oversize → U+FFFD); curated named table; unknown/malformed → verbatim |
| Markdown | `core/src/ops/markdown.rs` | options `TABLES｜STRIKETHROUGH｜TASKLISTS`; text/code kept, formatting dropped; link text kept / URL dropped; image alt kept; soft break → space, hard break → `\n`; loose vs tight block spacing; table cells tab-separated; embedded HTML → `strip_html` |
| Whitespace | `core/src/ops/whitespace.rs` | `collapse_whitespace` only ASCII space/tab, never `\n`; `trim_trailing_whitespace` trims non-newline whitespace per line (CRLF→LF as a side effect) |
| Lines | `core/src/ops/lines.rs` | the shared line model (split on `\n`, strip trailing `\r` run); trailing-newline round-trip; `unwrap_lines` → clean paragraph block, no trailing newline; heuristic email/URL extraction |
| Case | `core/src/ops/case.rs` | full-Unicode mappings; **positional** title case (`3RD`→`3rd`, `(HELLO)`→`(hello)`); sentence = lowercase then capitalize after `.`/`!`/`?` + whitespace |

## Enforcing checks

- **Property + regression tests:** `cargo test -p safetystrip-core`. New behavior
  needs both a regression test (the right answer) **and** adversarial-input coverage.
- **Determinism property test:** `transform(x,c) == transform(x,c)`.
- **Fuzzing (never-panics):** run the target(s) covering what you changed —
  `cargo +nightly fuzz run strip_html | strip_markdown | transform_pipeline`. Commit
  any crashing input found under `fuzz/` so it replays as a regression. (CI runs a
  short best-effort fuzz smoke; the required signal is the property/corpus tests.)
- **Lints:** `cargo clippy -p safetystrip-core --all-targets -- -D warnings`,
  `cargo fmt`.

## What a PR must call out

- Any behavior change to a documented rule (and the matching doc-comment + test
  update). State it explicitly — output changes are a contract change.
- New adversarial cases covered, and the fuzz target(s) you ran.
- Confirmation the change is core-only data (no ABI change, no new dependency
  outside the allowlist).
