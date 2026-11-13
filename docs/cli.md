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
mirqab open [TAB] [--window] [--dry-run]
```

Shows the window, on TAB if given. --window opens a normal window instead of the popover. --dry-run prints what the daemon would do and changes nothing.

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

Serves the Model Context Protocol on stdin and stdout until stdin closes, for a coding agent: the tools read the daemon (system_load, explain_slowness). Register it with `mirqab mcp-config CLIENT`.

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
