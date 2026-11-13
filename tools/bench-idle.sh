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
shopt -s inherit_errexit
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
for v in "$seconds" "$runs"; do
    [[ $v =~ ^[1-9][0-9]*$ ]] || { echo "bench-idle: --seconds and --runs take a whole number from 1" >&2; exit 64; }
done
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
# The daemon and the socat holding the subscription are children of this
# shell, never of a command substitution, so the trap below always knows
# them (review of PR #6: both leaked on every early exit).
daemon="" holder="" fd_open=0
stop_children() {
    if ((fd_open)); then exec 3>&-; fd_open=0; fi
    [[ -z $holder ]] || { kill "$holder" 2>/dev/null || true; wait "$holder" 2>/dev/null || true; holder=""; }
    [[ -z $daemon ]] || { kill "$daemon" 2>/dev/null || true; wait "$daemon" 2>/dev/null || true; daemon=""; }
}
cleanup() {
    stop_children
    rm -rf -- "$scratch"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# ticks PID — utime + stime of the whole process.
ticks() { awk '{ print $14 + $15 }' "/proc/$1/stat"; }
# switches PID — voluntary + involuntary context switches of every thread
# alive now. Approximate: a thread that exits between two readings takes
# its count with it, so the difference can even be negative. Printed, not
# judged.
switches() {
    cat /proc/"$1"/task/*/status 2>/dev/null |
        awk '/^(voluntary|nonvoluntary)_ctxt_switches:/ { n += $2 } END { print n + 0 }'
}

# fail MESSAGE — stop the run's children and report that it cannot measure.
fail() {
    echo "bench-idle: $*" >&2
    stop_children
    return 2
}

# measure N — one run; sets cpu (percent of one core), rss_kib and ctx.
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
        kill -0 "$daemon" 2>/dev/null || { cat "$run/err" >&2; fail "the daemon exited at start"; return; }
        sleep 0.05
    done
    [[ -S $sock ]] || { fail "the daemon did not start"; return; }
    # "Window closed" is part of the measurement: no interface may use this
    # socket. Best effort: an interface that found the socket through
    # MIRQAB_SOCKET has no trace of it on its command line, but the socket
    # is new, so nothing else knows it.
    local p
    for p in $(pgrep -x mirqab-ui || true); do
        if tr '\0' '\n' <"/proc/$p/cmdline" 2>/dev/null | grep -qxF -- "$sock"; then
            fail "a mirqab-ui (pid $p) is attached to $sock"
            return
        fi
    done
    if [[ -n $stream ]]; then
        local params='{"interval":2000}'
        ((live == 0)) || params='{"interval":1000,"live":true}'
        # socat reads the request from a fifo held open on fd 3, so the
        # connection lasts until the fd closes; no sleeping helper to leak.
        mkfifo "$run/sub"
        socat - "UNIX-CONNECT:$sock" <"$run/sub" >"$run/sub.out" 2>"$run/sub.err" &
        holder=$!
        exec 3>"$run/sub"
        fd_open=1
        printf '{"id":1,"method":"subscribe","stream":"%s","params":%s}\n' "$stream" "$params" >&3
    fi
    sleep 5
    if [[ -n $stream ]]; then
        # The subscription must have taken: socat alive, and a reply with a
        # result to request 1 (an unknown stream measures idle otherwise).
        kill -0 "$holder" 2>/dev/null || { cat "$run/sub.err" >&2; fail "socat could not hold the subscription"; return; }
        grep -q '"id":1,"result"' "$run/sub.out" ||
            { grep -m1 '"id":1' "$run/sub.out" >&2 || true; fail "the subscription to $stream was refused"; return; }
    fi
    local t0 s0 t1 s1
    t0=$(ticks "$daemon") s0=$(switches "$daemon")
    sleep "$seconds"
    # A daemon that died during the minute is a failure, not an idle one.
    kill -0 "$daemon" 2>/dev/null || { fail "the daemon exited during the measurement"; return; }
    t1=$(ticks "$daemon") s1=$(switches "$daemon")
    rss_kib=$(awk '/^VmRSS:/ { print $2 }' "/proc/$daemon/status")
    # Each reading checked: on one line, only the last substitution's
    # status counts, so a failed first reading would pass unseen.
    [[ -n $t0 && -n $t1 && -n $rss_kib ]] || { fail "could not read the daemon's /proc entries"; return; }
    stop_children
    cpu=$(awk -v d=$((t1 - t0)) -v s="$seconds" -v t="$tck" 'BEGIN { printf "%.3f", d * 100 / (s * t) }')
    ctx=$((s1 - s0))
}

mib() { awk -v k="$1" 'BEGIN { printf "%.1f", k / 1024 }'; }

best_cpu="" best_rss=""
for ((n = 1; n <= runs; n++)); do
    cpu="" rss_kib="" ctx=""
    measure "$n" || exit 2
    label="no subscriber"
    [[ -z $stream ]] || label="subscribed to $stream$( ((live)) && echo ' (live)')"
    printf 'run %d/%d, %s: cpu %s%% of one core over %ss, rss %s MiB, %s context switches\n' \
        "$n" "$runs" "$label" "$cpu" "$seconds" "$(mib "$rss_kib")" "$ctx"
    if [[ -z $best_cpu ]] || awk -v a="$cpu" -v b="$best_cpu" 'BEGIN { exit !(a < b) }'; then
        best_cpu=$cpu best_rss=$rss_kib
    fi
done

# RSS is judged in KiB, unrounded: 14.97 MiB must not print as 15.0 and fail.
if awk -v c="$best_cpu" -v mc="$max_cpu" -v r="$best_rss" -v mr="$max_rss" \
    'BEGIN { exit !(c < mc && r < mr * 1024) }'; then
    printf 'within budget: cpu %s%% < %s%%, rss %s MiB < %s MiB\n' "$best_cpu" "$max_cpu" "$(mib "$best_rss")" "$max_rss"
else
    printf 'OVER BUDGET: cpu %s%% (budget %s%%), rss %s MiB (budget %s MiB)\n' \
        "$best_cpu" "$max_cpu" "$(mib "$best_rss")" "$max_rss" >&2
    exit 1
fi
