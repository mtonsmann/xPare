# Fuzz regressions

Minimized reproducers for findings from the fuzz targets, one subdirectory per
target. Unlike `fuzz/corpus/` and `fuzz/artifacts/` (both gitignored, machine-local),
this directory is **checked in** so a crash found once is replayed forever.

`scripts/overnight-fuzz.sh` populates it automatically: when a campaign produces a new
crash/oom/timeout artifact that still fails when re-run single-threaded (i.e. not a
contention artifact), the script minimizes it (`cargo fuzz tmin`), decodes it
(`cargo fuzz fmt`), and writes here:

- `<target>/<kind>-<hash>` — the minimized reproducer bytes.
- `<target>/<kind>-<hash>.repro.md` — toolchain, commit, repro command, decoded input,
  and the failure signature.

Run `scripts/overnight-fuzz.sh --auto-commit …` to commit these as it finds them, or
commit the printed one-liner yourself.

## Replaying

Reproduce a single finding:

```sh
cargo +nightly fuzz run <target> fuzz/regressions/<target>/<file> -- -runs=1
```

Replay every committed reproducer for a target once, without mutation (a regression
gate — confirms past findings stay fixed):

```sh
cargo +nightly fuzz run <target> fuzz/regressions/<target> -- -runs=0
```

When a finding is fixed, keep its reproducer here as a permanent guard.
