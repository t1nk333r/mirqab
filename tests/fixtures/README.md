# Fixtures

Synthetic trees that stand for `/` (architecture §5.3). Every value is made
up: no file here was copied from a live machine.

| Tree | What |
|------|------|
| `root-desktop/` | A desktop: 16 CPUs (8 physical) with cpufreq, 64 GB, zram, pressure, vmstat, hwmon (k10temp, two NVMe, two amdgpu, spd5118, one with no temperature), two AMD cards, a readable RAPL counter, `proc/self` for the daemon's own cost; about forty processes under `proc/<pid>/` (plan 006), `etc/passwd`, and desktop entries under `usr/share/applications/` and `var/lib/flatpak/exports/` |
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

`bin/pacman` is a stub of pacman for the process inspector: tests copy it
into a scratch directory on `PATH` or hand its path to the sampler.
