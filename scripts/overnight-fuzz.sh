#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
Usage: scripts/overnight-fuzz.sh [OPTIONS] HOURS [TARGET ...]

Runs local cargo-fuzz campaigns sized from current CPU load and available memory,
then triages any new findings (minimize + decode + stage a committable reproducer).

Arguments:
  HOURS   Runtime in hours. Decimals are accepted, e.g. 0.5 or 8.
  TARGET  Optional fuzz target(s). Defaults to every target from `cargo fuzz list`.

Options:
  --auto-commit   Commit each confirmed reproducer + triage note automatically (on a
                  fresh branch if you are on main/master). Default: print the commit
                  one-liner and leave the files staged for you.
  --no-triage     Just fuzz; skip the minimize/decode/stage step.
  -h, --help      Show this help.

Environment:
  FUZZ_LOAD_PERCENT            Target system load, default 85. Keep this 80-90.
  FUZZ_MIN_FREE_MIB_PER_WORKER Available-memory budget per worker, default 512.
  FUZZ_RESERVE_MIB             Memory to leave untouched, default 2048.
  FUZZ_RSS_LIMIT_MIB           Per-worker RSS cap passed to libFuzzer (-rss_limit_mb).
                               Default: sized so workers cannot overcommit RAM (see
                               below). A worker that exceeds it is a real OOM finding.
  FUZZ_ALLOW_OVERCOMMIT=1      Run one worker even when current load is already high.
  FUZZ_DRY_RUN=1               Print the selected targets/workers and exit.

Triage: after each target, any new crash/oom/timeout artifact is re-run single-threaded
to filter contention artifacts (a unit flagged during a saturated multi-worker campaign
may just have been starved, not buggy). A finding that still fails on one core is
minimized with `cargo fuzz tmin`, decoded with `cargo fuzz fmt`, and copied — with a
triage note (toolchain, commit, repro command, decoded input, failure signature) — into
`fuzz/regressions/<target>/`. The per-worker RSS cap below stops the worst inflation
source (memory overcommit -> swap); the single-threaded re-confirm covers the rest.
USAGE
}

# --- argument parsing: separate flags from positionals (HOURS + targets) ----------
auto_commit=0
triage=1
positional=""
for arg in "$@"; do
    case "$arg" in
        -h | --help) usage; exit 0 ;;
        --auto-commit) auto_commit=1 ;;
        --no-triage) triage=0 ;;
        --*) printf 'error: unknown option: %s\n' "$arg" >&2; usage; exit 2 ;;
        *) positional="${positional:+$positional }$arg" ;;
    esac
done
# Intentional word-split: HOURS is numeric and target names are simple identifiers.
# shellcheck disable=SC2086
set -- $positional

hours="${1:-}"

if [ -z "$hours" ]; then
    usage
    exit 2
fi
shift

case "$hours" in
    *[!0-9.]* | "" | "." | *.*.*)
        echo "error: HOURS must be a positive number" >&2
        exit 2
        ;;
esac

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fuzz_dir="$repo_root/fuzz"
log_dir="$repo_root/fuzz-runs"
artifacts_dir="$fuzz_dir/artifacts"
regress_dir="$fuzz_dir/regressions"
cd "$fuzz_dir"

if [ "$#" -gt 0 ]; then
    targets="$*"
else
    targets=$(cargo +nightly fuzz list)
fi

target_count=$(printf '%s\n' "$targets" | awk 'NF { count++ } END { print count + 0 }')
if [ "$target_count" -lt 1 ]; then
    echo "error: no fuzz targets found" >&2
    exit 1
fi

cores=$(
    sysctl -n hw.ncpu 2>/dev/null ||
        getconf _NPROCESSORS_ONLN 2>/dev/null ||
        echo 1
)

load_percent="${FUZZ_LOAD_PERCENT:-85}"
min_free_mib_per_worker="${FUZZ_MIN_FREE_MIB_PER_WORKER:-512}"
reserve_mib="${FUZZ_RESERVE_MIB:-2048}"

current_load=$(
    uptime | awk -F'load averages?: |load average: ' '{ split($2, avg, /,? +/); print avg[1] }'
)

workers=$(
    awk -v cores="$cores" -v pct="$load_percent" '
        BEGIN {
            desired = cores * pct / 100.0
            workers = int(desired)
            if (workers < desired) {
                workers += 1
            }
            if (workers > cores) {
                workers = cores
            }
            if (workers < 1) {
                workers = 1
            }
            print workers
        }
    '
)

available_mib=$(
    if command -v vm_stat >/dev/null 2>&1; then
        vm_stat | awk '
            /page size of/ { page = $8 }
            /Pages free:/ { gsub(/\./, "", $3); free = $3 }
            /Pages inactive:/ { gsub(/\./, "", $3); inactive = $3 }
            /Pages speculative:/ { gsub(/\./, "", $3); speculative = $3 }
            END {
                if (page > 0) {
                    print int((free + inactive + speculative) * page / 1048576)
                }
            }
        '
    fi
)

if [ -n "$available_mib" ]; then
    memory_workers=$(
        awk -v available="$available_mib" -v reserve="$reserve_mib" -v per="$min_free_mib_per_worker" '
            BEGIN {
                usable = available - reserve
                if (usable <= 0) {
                    print 0
                } else {
                    print int(usable / per)
                }
            }
        '
    )
    if [ "$memory_workers" -lt "$workers" ]; then
        workers="$memory_workers"
    fi
fi

if [ "$workers" -lt 1 ]; then
    if [ "${FUZZ_ALLOW_OVERCOMMIT:-0}" = "1" ]; then
        workers=1
    else
        cat >&2 <<EOF
Current system load is already near the requested target.

cores:        $cores
current load: $current_load
target load:  ${load_percent}%
available MiB:${available_mib:-unknown}

Set FUZZ_ALLOW_OVERCOMMIT=1 to force a one-worker run.
EOF
        exit 1
    fi
fi

# Per-worker RSS cap. The default libFuzzer limit is 2048 MiB *per worker*; with N
# workers that silently overcommits a smaller box, and the resulting swap inflates
# per-unit wall-clock into spurious slow-unit/oom artifacts (the contention false
# positives the triage note warns about). Size the cap so workers*cap fits the same
# usable-memory budget the worker count was derived from, so the campaign cannot swap.
# A worker that genuinely balloons past this cap is still killed and reported — that is
# a real out-of-memory finding (e.g. a pipeline-amplification regression), not noise.
rss_limit_mb="${FUZZ_RSS_LIMIT_MIB:-}"
if [ -z "$rss_limit_mb" ]; then
    if [ -n "$available_mib" ]; then
        rss_limit_mb=$(
            awk -v avail="$available_mib" -v reserve="$reserve_mib" -v workers="$workers" '
                BEGIN {
                    usable = avail - reserve
                    if (usable < 0) usable = 0
                    per = int(usable / workers)
                    if (per < 512) per = 512    # a worker needs working room
                    if (per > 2048) per = 2048  # never exceed libFuzzer'\''s own default
                    print per
                }
            '
        )
    else
        # No memory reading available (e.g. non-macOS): keep libFuzzer's default.
        rss_limit_mb=2048
    fi
fi

seconds=$(
    awk -v hours="$hours" 'BEGIN { seconds = int(hours * 3600); if (seconds < 1) seconds = 1; print seconds }'
)
seconds_per_target=$(
    awk -v seconds="$seconds" -v targets="$target_count" 'BEGIN { each = int(seconds / targets); if (each < 1) each = 1; print each }'
)

mkdir -p "$log_dir"
timestamp=$(date +%Y%m%d-%H%M%S)

cat <<EOF
Starting local fuzz campaign
targets:      $target_count
runtime:      ${hours}h total (${seconds}s), ${seconds_per_target}s per target
cores:        $cores
current load: $current_load
target load:  ${load_percent}%
workers/jobs: $workers
available MiB:${available_mib:-unknown}
rss cap/wkr:  ${rss_limit_mb} MiB (-rss_limit_mb)
triage:       $([ "$triage" = 1 ] && echo "on (auto-commit=$auto_commit)" || echo "off")
log dir:      $log_dir
EOF

if [ "${FUZZ_DRY_RUN:-0}" = "1" ]; then
    printf 'selected targets:\n'
    for target in $targets; do
        printf '  %s\n' "$target"
    done
    exit 0
fi

# --- triage helpers ----------------------------------------------------------------
# Scratch space for snapshots and tool logs; cleaned up on exit.
triage_tmp=$(mktemp -d "${TMPDIR:-/tmp}/xp-fuzz-triage.XXXXXX")
trap 'rm -rf "$triage_tmp"' EXIT

nightly_version=$(rustc +nightly --version 2>/dev/null || echo "unknown nightly toolchain")
repo_commit=$(git -C "$repo_root" rev-parse --short HEAD 2>/dev/null || echo "unknown")
confirmed_count=0
unconfirmed_count=0

# Genuine *failure* artifacts for a target: crash/oom/timeout. Deliberately NOT
# slow-unit — that is informational (the run continues) and the prime contention
# false-positive, handled by the single-threaded re-confirm note, not by committing.
list_findings() {
    _lf_dir="$artifacts_dir/$1"
    [ -d "$_lf_dir" ] || return 0
    find "$_lf_dir" -maxdepth 1 -type f \
        \( -name 'crash-*' -o -name 'oom-*' -o -name 'timeout-*' \) 2>/dev/null | sort
}

# Commit a reproducer + its note. Branch first if on the default branch (repo norm:
# never commit straight to main). Never pushes — that stays the human's call.
commit_finding() {
    _cf_repro="$1"; _cf_note="$2"; _cf_msg="$3"
    _cf_branch=$(git -C "$repo_root" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
    if [ "$_cf_branch" = "main" ] || [ "$_cf_branch" = "master" ]; then
        _cf_new="fuzz-findings-$timestamp"
        if ! git -C "$repo_root" rev-parse --verify --quiet "$_cf_new" >/dev/null 2>&1; then
            echo "    on $_cf_branch; creating branch $_cf_new for reproducer commits"
            if ! git -C "$repo_root" checkout -b "$_cf_new" >/dev/null 2>&1; then
                echo "    WARN: could not create branch; reproducer left staged" >&2
                return 0
            fi
        else
            git -C "$repo_root" checkout "$_cf_new" >/dev/null 2>&1 || true
        fi
    fi
    if git -C "$repo_root" add "$_cf_repro" "$_cf_note" \
        && git -C "$repo_root" commit -q -m "$_cf_msg"; then
        echo "    committed: $_cf_msg"
    else
        echo "    WARN: auto-commit failed; reproducer is staged at $_cf_repro" >&2
    fi
}

# Triage one new finding: re-confirm single-threaded, minimize, decode, stage.
triage_one() {
    _to_target="$1"; _to_finding="$2"; _to_log="$3"
    _to_base=$(basename "$_to_finding")
    _to_kind=${_to_base%%-*}                       # crash | oom | timeout
    _to_short=$(printf '%s' "${_to_base#*-}" | cut -c1-12)

    # 1. Re-confirm on a single core. If it runs clean, it was a contention artifact,
    #    not a bug — leave it in artifacts/ and do not commit.
    if cargo +nightly fuzz run "$_to_target" "$_to_finding" -- \
        -runs=1 -rss_limit_mb="$rss_limit_mb" > "$triage_tmp/confirm.log" 2>&1; then
        echo "    UNCONFIRMED: ran clean on one core — contention artifact, not committed."
        unconfirmed_count=$((unconfirmed_count + 1))
        return 0
    fi

    # 2. Minimize. -exact_artifact_path makes libFuzzer write the minimized crash to a
    #    known path so we don't have to guess tmin's output name.
    _to_min="$triage_tmp/min-$_to_target-$_to_short"
    cargo +nightly fuzz tmin "$_to_target" "$_to_finding" -- \
        -rss_limit_mb="$rss_limit_mb" -exact_artifact_path="$_to_min" \
        > "$triage_tmp/tmin.log" 2>&1 || true
    if [ ! -s "$_to_min" ]; then
        _to_min="$_to_finding"   # minimization produced nothing usable; keep the original
        echo "    note: minimization did not shrink the input; using the original."
    fi

    # 3. Decode (structured targets print their Arbitrary Debug; byte targets the bytes).
    _to_fmt="(cargo fuzz fmt produced no output)"
    if cargo +nightly fuzz fmt "$_to_target" "$_to_min" > "$triage_tmp/fmt.out" 2>/dev/null \
        && [ -s "$triage_tmp/fmt.out" ]; then
        _to_fmt=$(cat "$triage_tmp/fmt.out")
    fi

    # 4. Stage the minimized reproducer + a triage note under the tracked regress dir.
    mkdir -p "$regress_dir/$_to_target"
    _to_repro_rel="fuzz/regressions/$_to_target/${_to_kind}-${_to_short}"
    _to_repro="$repo_root/$_to_repro_rel"
    cp "$_to_min" "$_to_repro"
    _to_sig=$(grep -m1 -iE 'ERROR: libFuzzer|panicked|SUMMARY:|out-of-memory|deadly signal' \
        "$_to_log" 2>/dev/null || true)
    _to_note_rel="${_to_repro_rel}.repro.md"
    # The triple-backticks below are literal Markdown fences, not command
    # substitution — printf only ever expands its %s arguments.
    # shellcheck disable=SC2016
    {
        printf '# Fuzz regression: %s (%s)\n\n' "$_to_target" "$_to_kind"
        printf -- '- discovered: %s\n' "$timestamp"
        printf -- '- toolchain:  %s\n' "$nightly_version"
        printf -- '- commit:     %s\n' "$repo_commit"
        printf '\n## Reproduce\n\n```sh\ncargo +nightly fuzz run %s %s -- -runs=1 -rss_limit_mb=%s\n```\n' \
            "$_to_target" "$_to_repro_rel" "$rss_limit_mb"
        printf '\n## Decoded input (cargo fuzz fmt)\n\n```\n%s\n```\n' "$_to_fmt"
        if [ -n "$_to_sig" ]; then
            printf '\n## Failure signature\n\n```\n%s\n```\n' "$_to_sig"
        fi
    } > "$repo_root/$_to_note_rel"

    confirmed_count=$((confirmed_count + 1))
    echo "    CONFIRMED $_to_kind. Staged minimized reproducer + triage note:"
    echo "      $_to_repro_rel"
    echo "      $_to_note_rel"

    _to_msg="fuzz($_to_target): commit ${_to_kind} reproducer ${_to_short}"
    if [ "$auto_commit" = "1" ]; then
        commit_finding "$_to_repro_rel" "$_to_note_rel" "$_to_msg"
    else
        echo "    to commit this regression:"
        echo "      git add $_to_repro_rel $_to_note_rel \\"
        echo "        && git commit -m \"$_to_msg\""
    fi
}

# Triage every finding for a target that is new versus a pre-run snapshot.
triage_findings() {
    _tf_target="$1"; _tf_before="$2"; _tf_log="$3"
    # Snapshot the post-run findings once, so tmin's own output (written during
    # triage) is never picked up as a fresh finding on a later iteration.
    list_findings "$_tf_target" > "$triage_tmp/after.list"
    while IFS= read -r _tf_finding; do
        [ -n "$_tf_finding" ] || continue
        if grep -qxF "$_tf_finding" "$_tf_before"; then
            continue
        fi
        echo
        echo "==> New finding: $_tf_finding"
        triage_one "$_tf_target" "$_tf_finding" "$_tf_log"
    done < "$triage_tmp/after.list"
}

# --- fuzz + triage loop ------------------------------------------------------------
failed=0
for target in $targets; do
    log_file="$log_dir/${target}-${timestamp}.log"
    before_findings="$triage_tmp/before-$target.list"
    list_findings "$target" > "$before_findings" || true
    echo
    echo "==> Fuzzing $target for ${seconds_per_target}s with $workers worker(s)"
    echo "    log: $log_file"

    if ! cargo +nightly fuzz run "$target" -- \
        -workers="$workers" \
        -jobs="$workers" \
        -max_total_time="$seconds_per_target" \
        -rss_limit_mb="$rss_limit_mb" \
        -print_final_stats=1 \
        2>&1 | tee "$log_file"; then
        failed=1
    fi

    if [ "$triage" = "1" ]; then
        triage_findings "$target" "$before_findings" "$log_file"
    fi
done

if [ "$triage" = "1" ]; then
    echo
    echo "Triage: $confirmed_count confirmed, $unconfirmed_count unconfirmed (contention?)."
    if [ "$confirmed_count" -gt 0 ]; then
        echo "Minimized reproducers + notes are under fuzz/regressions/<target>/."
        [ "$auto_commit" = "1" ] || echo "Re-run with --auto-commit to commit them for you."
    fi
fi

exit "$failed"
