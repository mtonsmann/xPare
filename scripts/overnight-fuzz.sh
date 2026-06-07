#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
Usage: scripts/overnight-fuzz.sh HOURS [TARGET ...]

Runs local cargo-fuzz campaigns sized from current CPU load and available memory.

Arguments:
  HOURS   Runtime in hours. Decimals are accepted, e.g. 0.5 or 8.
  TARGET  Optional fuzz target(s). Defaults to every target from `cargo fuzz list`.

Environment:
  FUZZ_LOAD_PERCENT            Target system load, default 85. Keep this 80-90.
  FUZZ_MIN_FREE_MIB_PER_WORKER Available-memory budget per worker, default 512.
  FUZZ_RESERVE_MIB             Memory to leave untouched, default 2048.
  FUZZ_RSS_LIMIT_MIB           Per-worker RSS cap passed to libFuzzer (-rss_limit_mb).
                               Default: sized so workers cannot overcommit RAM (see
                               below). A worker that exceeds it is a real OOM finding.
  FUZZ_ALLOW_OVERCOMMIT=1      Run one worker even when current load is already high.
  FUZZ_DRY_RUN=1               Print the selected targets/workers and exit.

Triage note: a unit flagged as slow (or an oom) during a saturated multi-worker
campaign may be a CONTENTION artifact, not an algorithmic bug — many workers sharing
the box inflate per-unit wall-clock. Always re-confirm a slow-unit/oom single-threaded
before treating it as a finding:
  cargo +nightly fuzz run TARGET artifacts/TARGET/slow-unit-... -- -runs=1
The per-worker RSS cap below stops the worst inflation source (memory overcommit ->
swap); it does not eliminate CPU oversubscription, so the re-confirm step still stands.
USAGE
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    usage
    exit 0
fi

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
log dir:      $log_dir
EOF

if [ "${FUZZ_DRY_RUN:-0}" = "1" ]; then
    printf 'selected targets:\n'
    for target in $targets; do
        printf '  %s\n' "$target"
    done
    exit 0
fi

failed=0
for target in $targets; do
    log_file="$log_dir/${target}-${timestamp}.log"
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
done

exit "$failed"
