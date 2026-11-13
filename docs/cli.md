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

Prints the version, the socket, each stream's subscribers and interval, and what the daemon costs.

## open

```
mirqab open [TAB] [--window]
```

Shows the window, on TAB if given. --window opens a normal window instead of the popover.

## toggle

```
mirqab toggle [TAB]
```

Shows the window, or hides it if it is shown.

## close

```
mirqab close
```

Hides the window.

## history

```
mirqab history SERIES... [--range R] [--json]
mirqab history --list
```

Prints the average and peak of each bucket of stored minutes. R is 1h (default), 24h, 7d or 30d. --list names the series history keeps.

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

## bar

```
mirqab bar ITEM [--watch] [--interval SEC] [--format waybar|plain|json]
mirqab bar --list
```

Prints one reading of ITEM for a status bar and exits. With --watch it prints a line each time the reading changes, at most once per interval (default 5 s, from 2 s to an hour), until its output is closed. --format waybar (default) prints Waybar's JSON; plain the text only; json every field.

## completions

```
mirqab completions bash|zsh|fish
```

Prints a completion script for the shell. For bash: mirqab completions bash > ~/.local/share/bash-completion/completions/mirqab

## help

```
mirqab help [COMMAND]
mirqab help --markdown
```

Prints this text, or one command's. --markdown prints every command as Markdown (docs/cli.md is made from it).

## Options for every command

- `--socket PATH`: the daemon's socket (default $XDG_RUNTIME_DIR/mirqab/daemon.sock, or $MIRQAB_SOCKET)
- `--help`: help for the command
- `--version`: print the version

Exit codes: 0 done, 1 refused or failed, 2 no daemon, 64 usage error.
