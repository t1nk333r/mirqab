#!/usr/bin/env bash
# tools/mkfixture.sh — writes generated fixture files from readable tables.
#
#   tools/mkfixture.sh procs [--out DIR]
#       From tests/fixtures/procs.tsv, writes proc/<pid>/ for every process
#       under DIR/root-desktop and DIR/root-desktop.t1 (default
#       tests/fixtures), and proc/self/ns. Every numbered directory under
#       those proc/ trees is removed first, so a row deleted from the table
#       leaves nothing behind. tests/test-fixtures.sh runs it into a scratch
#       directory and compares the result with the committed trees.
#
# What it writes, per process (plan 006, "Tests and fixtures"): stat, statm,
# status, cmdline and cgroup for everyone; for the user's own processes
# (uid 1000, as proc/self/status says) also io, cwd, ns/*, the exe link,
# and the fd, fdinfo, smaps_rollup and root/.flatpak-info the extras ask
# for. Another user's process gets no exe, io, cwd, ns or fd, as the kernel
# refuses those (EACCES).
set -Eeuo pipefail
cd "$(dirname "$0")/.."

OWN_UID=1000
HOME_DIR=/users/user
# The daemon's own namespaces (proc/self/ns); a process differs where its
# row's ns= says so.
NS_NAMES=(cgroup ipc mnt net pid user uts)
declare -A NS_ID=([cgroup]=4026531835 [ipc]=4026531839 [mnt]=4026531841
    [net]=4026531840 [pid]=4026531836 [user]=4026531837 [uts]=4026531838)
# What a renderD node's fd links to.
DRM_NODE=/dev/dri/renderD128

usage() { sed -n '2,10s/^# \{0,1\}//p' "$0"; }

state_word() {
    case $1 in
    R) echo running ;;
    S) echo sleeping ;;
    D) echo "disk sleep" ;;
    Z) echo zombie ;;
    T) echo stopped ;;
    I) echo idle ;;
    *) echo unknown ;;
    esac
}

# write_ns DIR [DIFFERING] — ns/* links in DIR; the names in DIFFERING
# (`mnt+pid`) get an id of their own.
write_ns() {
    local dir=$1 differ=${2:-} n id
    mkdir -p "$dir/ns"
    for n in "${NS_NAMES[@]}"; do
        id=${NS_ID[$n]}
        [[ +$differ+ == *"+$n+"* ]] && id=$((id + 1000))
        ln -s "$n:[$id]" "$dir/ns/$n"
    done
}

# write_proc TREE ROW... — one process's files under TREE/proc/<pid>.
write_proc() {
    local tree=$1 pid=$2 ppid=$3 uid=$4 state=$5 comm=$6 pgrp=$7 sid=$8 tpgid=$9
    local start=${10} ut=${11} st=${12} thr=${13} rss=${14} shr=${15} cgroup=${16}
    local exe=${17} cmdline=${18} extras=${19}
    local d="$tree/proc/$pid" tty=0 flags=4194560 nnp=0 seccomp=0
    local capeff=0000000000000000 cwd=$HOME_DIR ns="" io="" smaps="" flatpak="" kthread=0
    local fds=() drms=() e
    [[ $tpgid == - ]] && tpgid=-1 || tty=34816
    IFS=';' read -ra parts <<<"${extras/#-/}"
    for e in "${parts[@]}"; do
        case $e in
        kthread) kthread=1 ;;
        nnp) nnp=1 ;;
        seccomp=*) seccomp=${e#*=} ;;
        capeff=*) capeff=${e#*=} ;;
        cwd=*) cwd=${e#*=} ;;
        ns=*) ns=${e#*=} ;;
        io=*) io=${e#*=} ;;
        smaps=*) smaps=${e#*=} ;;
        flatpak=*) flatpak=${e#*=} ;;
        fd=*) fds+=("${e#*=}") ;;
        drm=*) drms+=("${e#*=}") ;;
        *) echo "mkfixture: pid $pid: unknown extra: $e" >&2; return 1 ;;
        esac
    done
    # PF_KTHREAD (0x00200000) marks a kernel thread.
    ((kthread)) && flags=2129984
    [[ $cgroup == "~"* ]] && cgroup="/user.slice/user-$uid.slice/user@$uid.service${cgroup:1}"
    mkdir -p "$d"

    # stat: 52 fields; after rsslim (field 25) every field is zero.
    local tail="" i
    for ((i = 26; i <= 52; i++)); do tail+=" 0"; done
    printf '%s (%s) %s %s %s %s %s %s %s 0 0 0 0 %s %s 0 0 20 0 %s 0 %s %s %s 18446744073709551615%s\n' \
        "$pid" "$comm" "$state" "$ppid" "$pgrp" "$sid" "$tty" "$tpgid" "$flags" \
        "$ut" "$st" "$thr" "$start" "$((rss * 3 * 4096))" "$rss" "$tail" >"$d/stat"
    printf '%s %s %s 100 0 %s 0\n' "$((rss * 3))" "$rss" "$shr" "$((rss / 2))" >"$d/statm"
    {
        printf 'Name:\t%s\nUmask:\t0022\nState:\t%s (%s)\n' "$comm" "$state" "$(state_word "$state")"
        printf 'Tgid:\t%s\nNgid:\t0\nPid:\t%s\nPPid:\t%s\nTracerPid:\t0\n' "$pid" "$pid" "$ppid"
        printf 'Uid:\t%s\t%s\t%s\t%s\nGid:\t%s\t%s\t%s\t%s\n' "$uid" "$uid" "$uid" "$uid" "$uid" "$uid" "$uid" "$uid"
        printf 'FDSize:\t64\nVmRSS:\t%s kB\nThreads:\t%s\n' "$((rss * 4))" "$thr"
        printf 'NoNewPrivs:\t%s\nSeccomp:\t%s\n' "$nnp" "$seccomp"
        printf 'CapInh:\t0000000000000000\nCapPrm:\t%s\nCapEff:\t%s\nCapBnd:\t000001ffffffffff\n' "$capeff" "$capeff"
    } >"$d/status"
    printf '0::%s\n' "$cgroup" >"$d/cgroup"
    # cmdline: NUL-separated with a trailing NUL; empty for a kernel thread.
    if [[ $cmdline == - ]]; then
        : >"$d/cmdline"
    else
        local args
        read -ra args <<<"$cmdline"
        printf '%s\0' "${args[@]}" >"$d/cmdline"
    fi

    ((uid == OWN_UID)) || return 0
    local r=0 w=0
    [[ -z $io ]] || IFS=: read -r r w _ <<<"$io"
    printf 'rchar: %s\nwchar: %s\nsyscr: 0\nsyscw: 0\nread_bytes: %s\nwrite_bytes: %s\ncancelled_write_bytes: 0\n' \
        "$r" "$w" "$r" "$w" >"$d/io"
    ln -s "$cwd" "$d/cwd"
    write_ns "$d" "$ns"
    [[ $exe == - ]] || ln -s "$exe" "$d/exe"
    if ((${#fds[@]} + ${#drms[@]})); then
        mkdir -p "$d/fd" "$d/fdinfo"
        local n target client vram gtt cpu
        for e in "${fds[@]}"; do
            n=${e%%:*}
            target=${e#*:}
            ln -s "$target" "$d/fd/$n"
            printf 'pos:\t0\nflags:\t02\nmnt_id:\t20\nino:\t%s\n' "$((n + 5000))" >"$d/fdinfo/$n"
        done
        for e in "${drms[@]}"; do
            IFS=: read -r n client vram gtt cpu <<<"$e"
            ln -s "$DRM_NODE" "$d/fd/$n"
            printf 'pos:\t0\nflags:\t02100002\nmnt_id:\t26\nino:\t1050\n' >"$d/fdinfo/$n"
            printf 'drm-driver:\tamdgpu\ndrm-client-id:\t%s\ndrm-pdev:\t0000:03:00.0\n' "$client" >>"$d/fdinfo/$n"
            printf 'drm-memory-vram:\t%s KiB\ndrm-memory-gtt:\t%s KiB\ndrm-memory-cpu:\t%s KiB\n' \
                "$vram" "$gtt" "$cpu" >>"$d/fdinfo/$n"
        done
    fi
    if [[ -n $smaps ]]; then
        local pss swap
        IFS=: read -r pss swap <<<"$smaps"
        printf '00400000-7ffd00000000 ---p 00000000 00:00 0                          [rollup]\n' >"$d/smaps_rollup"
        printf 'Rss:\t%s kB\nPss:\t%s kB\nShared_Clean:\t0 kB\nPrivate_Dirty:\t0 kB\nSwap:\t%s kB\nSwapPss:\t%s kB\n' \
            "$((rss * 4))" "$pss" "$swap" "$swap" >>"$d/smaps_rollup"
    fi
    if [[ -n $flatpak ]]; then
        mkdir -p "$d/root"
        printf '[Application]\nname=%s\nruntime=runtime/org.example.Platform/x86_64/1\n' "$flatpak" >"$d/root/.flatpak-info"
    fi
}

procs() {
    local out=tests/fixtures
    while (($#)); do
        case $1 in
        --out)
            out=${2:?--out needs a directory}
            shift 2
            ;;
        *) usage >&2; exit 64 ;;
        esac
    done
    local t0="$out/root-desktop" t1="$out/root-desktop.t1" t
    for t in "$t0" "$t1"; do
        mkdir -p "$t/proc/self"
        find "$t/proc" -mindepth 1 -maxdepth 1 -name '[0-9]*' -exec rm -rf -- {} +
        rm -rf -- "$t/proc/self/ns"
        write_ns "$t/proc/self"
    done
    local tree pid ppid uid state comm pgrp sid tpgid start ut st ut1 st1 thr rss shr cgroup exe cmdline extras
    while IFS=$'\t' read -r tree pid ppid uid state comm pgrp sid tpgid start ut st ut1 st1 thr rss shr cgroup exe cmdline extras; do
        [[ -z $tree || $tree == '#'* ]] && continue
        if [[ $tree == both || $tree == t0 ]]; then
            write_proc "$t0" "$pid" "$ppid" "$uid" "$state" "$comm" "$pgrp" "$sid" "$tpgid" "$start" \
                "$ut" "$st" "$thr" "$rss" "$shr" "$cgroup" "$exe" "$cmdline" "$extras"
        fi
        if [[ $tree == both || $tree == t1 ]]; then
            # Two seconds later: the CPU times and the io counters moved.
            local ex1=$extras
            if [[ $extras =~ (^|;)io=([0-9]+):([0-9]+):([0-9]+):([0-9]+) ]]; then
                ex1=${extras/"io=${BASH_REMATCH[2]}:${BASH_REMATCH[3]}"/"io=${BASH_REMATCH[4]}:${BASH_REMATCH[5]}"}
            fi
            write_proc "$t1" "$pid" "$ppid" "$uid" "$state" "$comm" "$pgrp" "$sid" "$tpgid" "$start" \
                "$ut1" "$st1" "$thr" "$rss" "$shr" "$cgroup" "$exe" "$cmdline" "$ex1"
        fi
    done <tests/fixtures/procs.tsv
}

case ${1:-} in
procs)
    shift
    procs "$@"
    ;;
-h | --help) usage ;;
*) usage >&2; exit 64 ;;
esac
