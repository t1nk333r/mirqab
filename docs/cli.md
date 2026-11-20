# mirqab command line

Generated from the command table in `crates/mirqab-proto/src/strings.rs`
by `mirqab help --markdown`; a test keeps this file in step.

## daemon

```
mirqab daemon [--root DIR] [--socket PATH]
```

Serves the socket. --root reads system state from DIR instead of / (tests use fixture trees). Run by hand, it prints `ready <socket>` once it listens.

## status

```
mirqab status [--json]
```

Prints the version, the socket, each stream's subscribers and interval, what the daemon costs, and the focus source the Apps page's "idle but heavy" tag uses (Hyprland, only while the page is open).

## open

```
mirqab open [TAB] [--window] [--dry-run]
```

Shows the window, on TAB if given. --window opens a normal window even when the config asks for a popover. --dry-run prints what the daemon would do and changes nothing.

## toggle

```
mirqab toggle [TAB] [--dry-run]
```

Shows the window, or hides it if it is shown. --dry-run prints what the daemon would do and changes nothing.

## close

```
mirqab close [--dry-run]
```

Hides the window. --dry-run prints what the daemon would do and changes nothing.

## history

```
mirqab history SERIES... [--range 1h|24h|7d|30d] [--json]
mirqab history --list
```

Prints the average and peak of each bucket of stored minutes. The range is 1h (default), 24h, 7d or 30d. --list names the series history keeps.

## load

```
mirqab load [--json]
```

CPU (over the last seconds), load averages, memory, swap, pressure and the thermal word.

## sensors

```
mirqab sensors [--json]
```

Every temperature sensor, each GPU, and package and battery power.

## apps

```
mirqab apps [--sort cpu|mem|name] [--all] [--json]
```

One row per app: CPU (percent of one core, over the last seconds), memory and how many processes, heaviest first. A browser's helpers count as the browser and a terminal's jobs as themselves; other users' processes are one System group, last. --sort orders by cpu (default), mem or name. --all lists every process under its app. On Hyprland, while the Apps page is open, an app that used at least 20 % of a core for five minutes with none of its windows in focus is tagged in an IDLE column, shown only when some row is tagged.

## what

```
mirqab what PID [--json]
```

Explains one process: a verdict in plain words (which package and repository provide it, Flatpak, container, not owned by any package, replaced since it started), then its parents, user, executable, unit, sandbox, memory and open files. Another user's process shows its name, CPU and memory only.

## quit

```
mirqab quit PID|NAME [-y] [--force] [--dry-run] [--json]
```

Asks a process to quit (SIGTERM), or with --force ends it (SIGKILL). A number is a pid; anything else is an app's name as mirqab apps shows it, matched whole and ignoring case, and quits every process of that app, its main process first. A user service is stopped with systemctl --user, so it does not restart; the list names the other services the stop takes with it, and warns before the sound server or a desktop portal is stopped. The stop is refused when one of those services holds a process mirqab never signals, and at the confirmation when a service that was not listed would now stop too; when systemd cannot say what depends on a service, the list says so. It prints what it will do and asks; -y goes ahead without asking; without a terminal and without -y it does nothing and exits 3; --dry-run prints the list and stops. With --json the list goes to stderr and stdout holds the daemon's answer. It exits 1 when anything failed, or when nothing was done because everything was left alone; a process that had already exited counts as done. Never stopped or signalled: pid 1, another user's processes, kernel threads, anything outside your user's session, the daemon, the session's compositor, your user manager, the session bus (also any process named dbus-daemon, dbus-broker or dbus-broker-launch), and the program asking and its parents. A rootless container is not quit; stop it with its runtime. When the action could not be written to mirqab's history, it is still done and a warning says so.

## ports

```
mirqab ports [--udp] [--json]
```

Lists every listening TCP port: the address it is bound to, how far it reaches (local: only this machine; all: every interface; lan, bridge, vpn, link or other: that network; unknown: an address on no interface, treated as reachable), the process holding it and how long it has run. A port of root's or another user's shows its owner and no process: mirqab cannot see whose it is. A line after the table names each port of yours that the network can reach. mirqab reads the bind address only and never your firewall, so a reachable port may still be blocked by one. --udp adds bound UDP sockets.

## port

```
mirqab port PORT [--proto tcp|udp] [--json]
```

Shows everything mirqab knows about what listens on PORT (TCP unless --proto udp): one block per listener with its reach in words, the process, what it serves and how long it has run, and the URL to open it. It exits 1 when nothing listens there.

## bar

```
mirqab bar ITEM [--watch] [--interval SEC] [--format waybar|plain|json]
mirqab bar --list
```

Prints one reading of ITEM for a status bar and exits. With --watch it prints a line each time the reading changes, at most once per interval (default 5 s, from 2 s to an hour), until its output is closed. --format waybar (default) prints Waybar's JSON; plain the text only; json every field.

## mcp

```
mirqab mcp
mirqab --mcp
```

Serves the Model Context Protocol on stdin and stdout until stdin closes, for a coding agent: the tools read the daemon (system_load, explain_slowness, top_apps, what_is_process). Register it with `mirqab mcp-config CLIENT`.

## mcp-config

```
mirqab mcp-config claude|codex|gemini|opencode|cursor|json [--write [-y]] [--dry-run]
```

Prints how to register `mirqab mcp` with CLIENT: the client's own command, or for cursor and json the JSON to add. --write runs the client's command after showing it and asking, and refuses cursor and json; -y skips the question; without a terminal and without -y it does nothing and exits 3. --dry-run shows what --write would run.

## completions

```
mirqab completions bash|zsh|fish
```

Prints a completion script for the shell. For bash: `mirqab completions bash > ~/.local/share/bash-completion/completions/mirqab`. For zsh, write it to a file named `_mirqab` in a directory on `$fpath`, such as `~/.zfunc/_mirqab` with `fpath+=(~/.zfunc)` before `compinit`. For fish: `mirqab completions fish > ~/.config/fish/completions/mirqab.fish`.

## help

```
mirqab help [COMMAND]
mirqab help --markdown
```

Prints this text, or one command's. --markdown prints every command as Markdown (docs/cli.md is made from it).

## Options for every command

- `--socket PATH`: the socket (default $MIRQAB_SOCKET, else $XDG_RUNTIME_DIR/mirqab/daemon.sock)
- `-h, --help`: help for the command
- `-V, --version`: print the version

Exit codes: 0 done, 1 refused or failed, 2 no daemon, 3 not confirmed, 64 usage error.
