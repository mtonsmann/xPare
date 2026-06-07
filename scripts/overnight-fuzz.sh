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
  FUZZ_MIN_FREE_MIB_PER_WORKER Available-memory budget per worker, default 1024.
  FUZZ_RESERVE_MIB             Memory to leave untouched, default 2048.
  FUZZ_ALLOW_OVERCOMMIT=1      Run one worker even when current load is already high.
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
min_free_mib_per_worker="${FUZZ_MIN_FREE_MIB_PER_WORKER:-1024}"
reserve_mib="${FUZZ_RESERVE_MIB:-2048}"

current_load=$(
    uptime | awk -F'load averages?: |load average: ' '{ split($2, avg, /,? +/); print avg[1] }'
)

workers=$(
    awk -v cores="$cores" -v current="$current_load" -v pct="$load_percent" '
        BEGIN {
            desired = cores * pct / 100.0
            spare = desired - current
            workers = int(spare)
            if (spare > 0 && workers < 1) {
                workers = 1
            }
            if (workers > cores) {
                workers = cores
            }
            if (workers < 0) {
                workers = 0
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
log dir:      $log_dir
EOF

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
        -print_final_stats=1 \
        2>&1 | tee "$log_file"; then
        failed=1
    fi
done

exit "$failed"
