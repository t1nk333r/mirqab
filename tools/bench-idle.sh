#!/usr/bin/env bash
# tools/bench-idle.sh — what the daemon costs (architecture §9.1).
#
#   tools/bench-idle.sh [--subscribe STREAM [--live]] [--seconds N] [--runs N]
#                       [--max-cpu PCT] [--max-rss MIB] [--bin DIR]
#
# Starts `mirqab daemon --root tests/fixtures/root-desktop` on a private
# socket with a temporary HOME, waits 5 s, reads utime and stime (fields 14
# and 15 of /proc/<pid>/stat), sleeps N seconds (60), reads them again and
# divides the difference by N × CLK_TCK. Prints the percentage of one core,
# VmRSS, and the context switches of all its threads over the same time.
# With --subscribe it holds one subscription to STREAM for the measurement
# (with --live, a live one at 1000 ms) and applies that row's budget.
#
# Budgets: under 0.3 % and 15 MiB with no subscriber; under 1 % with a live
# subscription. --max-cpu and --max-rss override them. With --runs N it
# measures N times and judges the best run (CI is a shared, noisy machine).
# Exits 1 over budget, 2 when it cannot measure (no socat for --subscribe,
# an interface attached to the socket, a daemon that does not start).
#
# --bin DIR uses DIR/mirqab; otherwise it builds the release binary.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

stream="" live=0 seconds=60 runs=1 max_cpu="" max_rss="" bin=""
while (($#)); do
    case $1 in
    --subscribe) stream=${2:?--subscribe needs a stream}; shift 2 ;;
    --live) live=1; shift ;;
    --seconds) seconds=${2:?}; shift 2 ;;
    --runs) runs=${2:?}; shift 2 ;;
    --max-cpu) max_cpu=${2:?}; shift 2 ;;
    --max-rss) max_rss=${2:?}; shift 2 ;;
    --bin) bin=${2:?}; shift 2 ;;
    -h | --help) sed -n '2,/^set -E/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
    *) echo "bench-idle: unknown option $1" >&2; exit 64 ;;
    esac
done
((live == 0)) || [[ -n $stream ]] || { echo "bench-idle: --live needs --subscribe" >&2; exit 64; }
if [[ -z $max_cpu ]]; then
    if ((live)); then max_cpu=1; else max_cpu=0.3; fi
fi
[[ -n $max_rss ]] || max_rss=15
if [[ -n $stream ]] && ! command -v socat >/dev/null 2>&1; then
    echo "bench-idle: --subscribe needs socat" >&2
    exit 2
fi

if [[ -z $bin ]]; then
    cargo build --release --locked --quiet -p mirqab --bin mirqab
    bin=target/release
fi
tck=$(getconf CLK_TCK)

# The socket path must stay under 108 bytes, so the scratch dir is in /tmp.
scratch="$(mktemp -d /tmp/mqbench.XXXXXX)"
daemon="" holder=""
cleanup() {
    [[ -z $holder ]] || kill "$holder" 2>/dev/null || true
    [[ -z $daemon ]] || kill "$daemon" 2>/dev/null || true
    wait 2>/dev/null || true
    rm -rf -- "$scratch"
}
trap cleanup EXIT

# ticks PID — utime + stime of the whole process.
ticks() { awk '{ print $14 + $15 }' "/proc/$1/stat"; }
# switches PID — voluntary + involuntary context switches of every thread.
switches() {
    cat /proc/"$1"/task/*/status 2>/dev/null |
        awk '/^(voluntary|nonvoluntary)_ctxt_switches:/ { n += $2 } END { print n + 0 }'
}

measure() {
    local run="$scratch/run$1"
    mkdir -p "$run" && mkdir -m 700 "$run/xdg"
    local sock="$run/xdg/mirqab/daemon.sock"
    env -i PATH=/usr/bin:/bin HOME="$run" XDG_RUNTIME_DIR="$run/xdg" \
        XDG_CONFIG_HOME="$run/config" XDG_STATE_HOME="$run/state" \
        "$bin/mirqab" daemon --root tests/fixtures/root-desktop >"$run/out" 2>"$run/err" &
    daemon=$!
    local i
    for ((i = 0; i < 100; i++)); do
        [[ -S $sock ]] && break
        kill -0 "$daemon" 2>/dev/null || { cat "$run/err" >&2; return 2; }
        sleep 0.05
    done
    [[ -S $sock ]] || { echo "bench-idle: the daemon did not start" >&2; return 2; }
    # "Window closed" is part of the measurement: no interface may use it.
    if pgrep -x mirqab-ui >/dev/null 2>&1 &&
        grep -qsF -- "$sock" /proc/"$(pgrep -x mirqab-ui | head -1)"/cmdline; then
        echo "bench-idle: a mirqab-ui is attached to $sock" >&2
        return 2
    fi
    if [[ -n $stream ]]; then
        local params='{"interval":2000}'
        ((live == 0)) || params='{"interval":1000,"live":true}'
        # Hold the connection open for the whole measurement; events go to
        # /dev/null.
        { printf '{"id":1,"method":"subscribe","stream":"%s","params":%s}\n' "$stream" "$params"
            sleep $((seconds + 10)); } | socat - "UNIX-CONNECT:$sock" >/dev/null 2>&1 &
        holder=$!
    fi
    sleep 5
    local t0 s0 t1 s1 rss
    t0=$(ticks "$daemon") s0=$(switches "$daemon")
    sleep "$seconds"
    t1=$(ticks "$daemon") s1=$(switches "$daemon")
    rss=$(awk '/^VmRSS:/ { print $2 }' "/proc/$daemon/status")
    [[ -z $holder ]] || { kill "$holder" 2>/dev/null || true; wait "$holder" 2>/dev/null || true; holder=""; }
    kill "$daemon" 2>/dev/null || true
    wait "$daemon" 2>/dev/null || true
    daemon=""
    awk -v d=$((t1 - t0)) -v s="$seconds" -v t="$tck" -v r="$rss" -v c=$((s1 - s0)) \
        'BEGIN { printf "%.3f %.1f %d\n", d * 100 / (s * t), r / 1024, c }'
}

best_cpu="" best_rss=""
for ((n = 1; n <= runs; n++)); do
    result="$(measure "$n")" || exit 2
    read -r cpu rss ctx <<<"$result"
    label="no subscriber"
    [[ -z $stream ]] || label="subscribed to $stream$( ((live)) && echo ' (live)')"
    printf 'run %d/%d, %s: cpu %s%% of one core over %ss, rss %s MiB, %s context switches\n' \
        "$n" "$runs" "$label" "$cpu" "$seconds" "$rss" "$ctx"
    if [[ -z $best_cpu ]] || awk -v a="$cpu" -v b="$best_cpu" 'BEGIN { exit !(a < b) }'; then
        best_cpu=$cpu best_rss=$rss
    fi
done

if awk -v c="$best_cpu" -v mc="$max_cpu" -v r="$best_rss" -v mr="$max_rss" \
    'BEGIN { exit !(c < mc && r < mr) }'; then
    printf 'within budget: cpu %s%% < %s%%, rss %s MiB < %s MiB\n' "$best_cpu" "$max_cpu" "$best_rss" "$max_rss"
else
    printf 'OVER BUDGET: cpu %s%% (budget %s%%), rss %s MiB (budget %s MiB)\n' \
        "$best_cpu" "$max_cpu" "$best_rss" "$max_rss" >&2
    exit 1
fi
