# Fixtures

Synthetic trees that stand for `/` (architecture §5.3). Every value is made
up: no file here was copied from a live machine.

| Tree | What |
|------|------|
| `root-desktop/` | A desktop: 16 CPUs, 64 GB, `proc/self` for the daemon's own cost |
| `root-desktop.t1/` | The same machine a minute later, so counter differences have a known answer |
| `root-laptop/` | 8 CPUs, 16 GB; later plans add a battery |
| `root-minimal/` | Only `proc/stat` and `proc/meminfo`: every sampler must survive missing files |
