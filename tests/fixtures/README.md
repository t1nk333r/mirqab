# Fixtures

Synthetic trees that stand for `/` (architecture §5.3). Every value is made
up: no file here was copied from a live machine.

| Tree | What |
|------|------|
| `root-desktop/` | A desktop: 16 CPUs (8 physical) with cpufreq, 64 GB, zram, pressure, vmstat, hwmon (k10temp, two NVMe, two amdgpu, spd5118, one with no temperature), two AMD cards, a readable RAPL counter, `proc/self` for the daemon's own cost; about fifty processes under `proc/<pid>/` (plan 006), `etc/passwd`, socket tables and an interface table (plan 007), and desktop entries under `usr/share/applications/` and `var/lib/flatpak/exports/` |
| `root-desktop.t1/` | The same machine a minute later, so counter differences have a known answer (20 % CPU, 600 KiB paged in, 60 W); its processes are the same two seconds later (CPU times and storage counters moved), with pid 7000 reused by a new process |
| `root-laptop/` | 8 CPUs as an Intel hybrid (4 P, 4 E), coretemp near critical, throttle counters, an Intel GPU (clock only), a runtime-suspended AMD dGPU that must not be read, a discharging battery |
| `root-minimal/` | Only `proc/stat` and `proc/meminfo`: every sampler must survive missing files |

## Generated files

`proc/<pid>/` and `proc/self/ns/` in `root-desktop` and `root-desktop.t1`
are written by `tools/mkfixture.sh procs` from the table
`procs.tsv`, which says what each process is and why it is there; edit the
table and run the generator, never the files. `tests/test-fixtures.sh`
checks the two agree. The links among them (`exe`, `cwd`, `fd/*`) name
paths of an imagined machine and are never followed: `Root::read_prefix`
refuses a link that leaves the tree.

The user is uid 1000 with the home `/users/user`, so the privacy scan's
home-path pattern (`tools/secrets.sh`) still catches real paths.

`root-desktop/run/mirqab-test/dependents/<unit>` is what the recording
sender answers when an action asks which units a stop of `<unit>` takes
with it (plan 006, part 3): one `<unit> <ActiveState>` a line, standing for
`systemctl --user show`, which a fixture root never runs. `pipewire.service`
takes `wireplumber.service` (active) and names `pipewire-pulse.service`
(inactive, so left out).

Ports (plan 007), hand-written and identical in `root-desktop` and
`root-desktop.t1`: `proc/net/tcp`, `tcp6`, `udp` and `udp6` in the
kernel's layout (each IPv4 address is one 32-bit word printed as a
little-endian machine holds it, so 192.0.2.10 is `0A0200C0`; the port is
big-endian hex). Listening: 127.0.0.1:3000 (pid 6410, an agent's dev
server), 0.0.0.0:8000 (6600, exposed), 192.0.2.10:9000 (6610, on the LAN
address), 127.0.0.1:8384 and 203.0.113.5:22000 (1300, a user service; the
second on the VPN address), 127.0.0.1:4096 (6500, an agent that listens),
`[::]:5173` (3020, dual-stack), root's sshd on 0.0.0.0:22 and `[::]:22`,
root's 8088 on both families (a Docker proxy), root's 631 on 127.0.0.1 and
`[::1]`, and 127.0.0.1:6379 of uid 999; UDP 127.0.0.53:53 (root) and
0.0.0.0:41234 (6610). The inodes match the `fd=` links of `procs.tsv`.
`proc/sys/net/ipv6/bindv6only` is 0. `run/mirqab-test/ifaddrs.json` is
the interface table a fixture root uses in place of `getifaddrs`, with
made-up names and documentation addresses (`lo`, `lan0` 192.0.2.10 and
2001:db8::10, `wlan0` 192.0.2.20, `br-example` 198.51.100.1, `vpn0`
203.0.113.5; `veth0` 198.51.100.17, a virtual link with no kind of its
own, so scope `other`; `lan0:1` 192.0.2.11, an alias label of `lan0`,
which the table strips before it reads `sys/class/net/lan0/`; no committed
socket binds these two), and `sys/class/net/<name>/` gives each its kind
(`type`, `uevent`, `device/`, `wireless/`, `bridge/`). `users/user/code/acme-web`,
`users/user/src/web` and `users/user/src/api` hold the `package.json` files
part 1b reads for projects and frameworks (git refuses a tracked `.git`
directory, so no fixture project has one).

`bin/pacman` is a stub of pacman for the process inspector: tests copy it
into a scratch directory on `PATH` or hand its path to the sampler.
