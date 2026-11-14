//! Every string a person reads, in the interface, the CLI and the daemon's
//! data (architecture §7.2). English only for now; a translation replaces
//! this module's body and touches no call site.

pub const APP_NAME: &str = "mirqab";

/// One command of the CLI: its name, its usage lines (after `mirqab `),
/// each with a one-line summary for the usage list, and the paragraph
/// `--help` adds. The usage text, each command's help, `docs/cli.md` and the
/// shell completions are all made from this table (plan 005); the flags and
/// the values completion offers are read from the usage lines, so the two
/// cannot disagree.
pub struct CliCommand {
    pub name: &'static str,
    pub usage: &'static [(&'static str, &'static str)],
    pub help: &'static str,
}

pub const CLI_COMMANDS: &[CliCommand] = &[
    CliCommand {
        name: "daemon",
        usage: &[(
            "daemon [--root DIR] [--socket PATH]",
            "run the daemon (systemd starts it from mirqab.socket)",
        )],
        help: "Serves the socket. --root reads system state from DIR instead of / (tests use fixture trees). Run by hand, it prints `ready <socket>` once it listens.",
    },
    CliCommand {
        name: "status",
        usage: &[(
            "status [--json]",
            "version, socket, subscribers and the daemon's cost",
        )],
        help: "Prints the version, the socket, each stream's subscribers and interval, and what the daemon costs.",
    },
    CliCommand {
        name: "open",
        usage: &[(
            "open [TAB] [--window] [--dry-run]",
            "show the window, on TAB",
        )],
        help: "Shows the window, on TAB if given. --window opens a normal window even when the config asks for a popover. --dry-run prints what the daemon would do and changes nothing.",
    },
    CliCommand {
        name: "toggle",
        usage: &[(
            "toggle [TAB] [--dry-run]",
            "show the window, or hide it if it is shown",
        )],
        help: "Shows the window, or hides it if it is shown. --dry-run prints what the daemon would do and changes nothing.",
    },
    CliCommand {
        name: "close",
        usage: &[("close [--dry-run]", "hide the window")],
        help: "Hides the window. --dry-run prints what the daemon would do and changes nothing.",
    },
    CliCommand {
        name: "history",
        usage: &[
            (
                "history SERIES... [--range 1h|24h|7d|30d] [--json]",
                "stored minutes (default range 1h)",
            ),
            ("history --list", "the series history keeps"),
        ],
        help: "Prints the average and peak of each bucket of stored minutes. The range is 1h (default), 24h, 7d or 30d. --list names the series history keeps.",
    },
    CliCommand {
        name: "load",
        usage: &[(
            "load [--json]",
            "CPU, load, memory, pressure and the thermal word",
        )],
        help: "CPU (over the last seconds), load averages, memory, swap, pressure and the thermal word.",
    },
    CliCommand {
        name: "sensors",
        usage: &[("sensors [--json]", "temperatures, GPUs and power")],
        help: "Every temperature sensor, each GPU, and package and battery power.",
    },
    CliCommand {
        name: "apps",
        usage: &[(
            "apps [--sort cpu|mem|name] [--all] [--json]",
            "apps by CPU or memory, helpers folded in",
        )],
        help: "One row per app: CPU (percent of one core, over the last seconds), memory and how many processes, heaviest first. A browser's helpers count as the browser and a terminal's jobs as themselves; other users' processes are one System group, last. --sort orders by cpu (default), mem or name. --all lists every process under its app.",
    },
    CliCommand {
        name: "what",
        usage: &[(
            "what PID [--json]",
            "what a process is and where it came from",
        )],
        help: "Explains one process: a verdict in plain words (which package and repository provide it, Flatpak, container, not owned by any package, replaced since it started), then its parents, user, executable, unit, sandbox, memory and open files. Another user's process shows its name, CPU and memory only.",
    },
    CliCommand {
        name: "bar",
        usage: &[
            (
                "bar ITEM [--watch] [--interval SEC] [--format waybar|plain|json]",
                "a reading for a status bar",
            ),
            ("bar --list", "the items a bar can show"),
        ],
        help: "Prints one reading of ITEM for a status bar and exits. With --watch it prints a line each time the reading changes, at most once per interval (default 5 s, from 2 s to an hour), until its output is closed. --format waybar (default) prints Waybar's JSON; plain the text only; json every field.",
    },
    CliCommand {
        name: "mcp",
        usage: &[
            (
                "mcp",
                "the MCP server for coding agents, on stdin and stdout",
            ),
            ("--mcp", "the same"),
        ],
        help: "Serves the Model Context Protocol on stdin and stdout until stdin closes, for a coding agent: the tools read the daemon (system_load, explain_slowness, top_apps, what_is_process). Register it with `mirqab mcp-config CLIENT`.",
    },
    CliCommand {
        name: "mcp-config",
        usage: &[(
            "mcp-config claude|codex|gemini|opencode|cursor|json [--write [-y]] [--dry-run]",
            "how to register the MCP server with an agent",
        )],
        help: "Prints how to register `mirqab mcp` with CLIENT: the client's own command, or for cursor and json the JSON to add. --write runs the client's command after showing it and asking, and refuses cursor and json; -y skips the question; without a terminal and without -y it does nothing and exits 3. --dry-run shows what --write would run.",
    },
    CliCommand {
        name: "completions",
        usage: &[("completions bash|zsh|fish", "a shell completion script")],
        help: "Prints a completion script for the shell. For bash: `mirqab completions bash > ~/.local/share/bash-completion/completions/mirqab`. For zsh, write it to a file named `_mirqab` in a directory on `$fpath`, such as `~/.zfunc/_mirqab` with `fpath+=(~/.zfunc)` before `compinit`. For fish: `mirqab completions fish > ~/.config/fish/completions/mirqab.fish`.",
    },
    CliCommand {
        name: "help",
        usage: &[
            ("help [COMMAND]", "this text, or one command's"),
            ("help --markdown", "every command as Markdown"),
        ],
        help: "Prints this text, or one command's. --markdown prints every command as Markdown (docs/cli.md is made from it).",
    },
];

/// The options every command takes.
pub const CLI_GLOBAL: &[(&str, &str)] = &[
    (
        "--socket PATH",
        "the socket (default $MIRQAB_SOCKET, else $XDG_RUNTIME_DIR/mirqab/daemon.sock)",
    ),
    ("-h, --help", "help for the command"),
    ("-V, --version", "print the version"),
];

/// Exit codes, for the usage text and `docs/cli.md`.
pub const CLI_EXITS: &str =
    "Exit codes: 0 done, 1 refused or failed, 2 no daemon, 3 not confirmed, 64 usage error.";

/// The column the usage list's summaries start at, and the widest a usage
/// line may be and still share its row with its summary.
const USAGE_COLUMN: usize = 40;
const USAGE_INDENT: usize = 2;
const USAGE_FITS: usize = USAGE_COLUMN - USAGE_INDENT - 1;

pub fn cli_command(name: &str) -> Option<&'static CliCommand> {
    CLI_COMMANDS.iter().find(|c| c.name == name)
}

/// What a flag's argument completes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliArg {
    /// A word from a fixed list (`--range 1h|24h|7d|30d`).
    Words(&'static str),
    /// A file (`PATH`).
    File,
    /// A directory (`DIR`).
    Dir,
    /// Anything; nothing to offer (`SEC`).
    Free,
}

/// A flag read from the usage lines, and its argument if it takes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CliFlag {
    pub name: &'static str,
    pub arg: Option<CliArg>,
}

fn arg_of(word: &'static str) -> CliArg {
    match word {
        "PATH" => CliArg::File,
        "DIR" => CliArg::Dir,
        w if w.contains('|') => CliArg::Words(w),
        _ => CliArg::Free,
    }
}

/// The flags of a command's usage lines, each once. A flag's argument is
/// the word after it inside the same brackets: `[--range 1h|24h]` takes one,
/// `[--json]` does not.
pub fn cli_flags(c: &CliCommand) -> Vec<CliFlag> {
    let mut out: Vec<CliFlag> = Vec::new();
    for (line, _) in c.usage {
        let words: Vec<&'static str> = line.split_whitespace().collect();
        // The first word is the command (or `--mcp`, its other spelling).
        for (i, w) in words.iter().enumerate().skip(1) {
            let bare = w.trim_start_matches('[').trim_end_matches(']');
            if !bare.starts_with('-') || out.iter().any(|f| f.name == bare) {
                continue;
            }
            let arg = match words.get(i + 1) {
                Some(next) if !w.ends_with(']') && !next.starts_with('[') => {
                    Some(arg_of(next.trim_end_matches(']')))
                }
                _ => None,
            };
            out.push(CliFlag { name: bare, arg });
        }
    }
    out
}

/// The flags every command takes, as completion offers them.
pub fn cli_global_flags() -> [CliFlag; 3] {
    [
        CliFlag {
            name: "--socket",
            arg: Some(CliArg::File),
        },
        CliFlag {
            name: "--help",
            arg: None,
        },
        CliFlag {
            name: "--version",
            arg: None,
        },
    ]
}

/// The words a command's positional argument completes to: a fixed list
/// (`bash|zsh|fish`), the commands for `help`, and the items for `bar`.
pub fn cli_positionals(c: &CliCommand) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (line, _) in c.usage {
        let Some(w) = line.split_whitespace().nth(1) else {
            continue;
        };
        let bare = w
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim_end_matches("...");
        match bare {
            "COMMAND" => out.extend(CLI_COMMANDS.iter().map(|c| c.name)),
            "ITEM" => out.extend(BAR_ITEMS.iter().map(|(name, _)| *name)),
            b if b.contains('|') && !b.starts_with('-') => out.extend(b.split('|')),
            _ => {}
        }
    }
    out
}

/// Every flag that takes an argument, from every command and the global
/// ones, each once. A test checks no two commands give one flag different
/// arguments.
fn flags_with_args() -> Vec<(&'static str, CliArg)> {
    let mut out: Vec<(&'static str, CliArg)> = Vec::new();
    let all = CLI_COMMANDS
        .iter()
        .flat_map(cli_flags)
        .chain(cli_global_flags());
    for f in all {
        if let Some(arg) = f.arg
            && !out.iter().any(|(n, _)| *n == f.name)
        {
            out.push((f.name, arg));
        }
    }
    out
}

/// The words completion offers after `mirqab COMMAND`: its positionals,
/// its flags and the global flags.
fn command_words(c: &CliCommand) -> String {
    let mut words = cli_positionals(c);
    words.extend(cli_flags(c).iter().map(|f| f.name));
    words.extend(cli_global_flags().iter().map(|f| f.name));
    words.join(" ")
}

fn global_words() -> String {
    cli_global_flags()
        .iter()
        .map(|f| f.name)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `mirqab --help`.
pub fn cli_usage() -> String {
    let mut out = String::from("usage: mirqab <command> [options]\n\ncommands:\n");
    let indent = " ".repeat(USAGE_INDENT);
    for c in CLI_COMMANDS {
        for (u, summary) in c.usage {
            if u.len() <= USAGE_FITS {
                out.push_str(&format!("{indent}{u:<USAGE_FITS$} {summary}\n"));
            } else {
                // Too wide to share a row: the summary goes under it, at the
                // summaries' column.
                let pad = " ".repeat(USAGE_COLUMN);
                out.push_str(&format!("{indent}{u}\n{pad}{summary}\n"));
            }
        }
    }
    out.push_str("\noptions for every command:\n");
    for (o, what) in CLI_GLOBAL {
        out.push_str(&format!("{indent}{o:<15} {what}\n"));
    }
    out.push('\n');
    out.push_str(CLI_EXITS);
    out.push('\n');
    out
}

/// `mirqab COMMAND --help`.
pub fn command_help(name: &str) -> Option<String> {
    let c = cli_command(name)?;
    let mut out = String::new();
    for (i, (u, _)) in c.usage.iter().enumerate() {
        out.push_str(if i == 0 {
            "usage: mirqab "
        } else {
            "       mirqab "
        });
        out.push_str(u);
        out.push('\n');
    }
    out.push_str(c.help);
    out.push('\n');
    Some(out)
}

/// `mirqab help --markdown`, which is `docs/cli.md`.
pub fn cli_markdown() -> String {
    let mut out = String::from(
        "# mirqab command line\n\n\
         Generated from the command table in `crates/mirqab-proto/src/strings.rs`\n\
         by `mirqab help --markdown`; a test keeps this file in step.\n",
    );
    for c in CLI_COMMANDS {
        out.push_str(&format!("\n## {}\n\n```\n", c.name));
        for (u, _) in c.usage {
            out.push_str(&format!("mirqab {u}\n"));
        }
        out.push_str("```\n\n");
        out.push_str(c.help);
        out.push('\n');
    }
    out.push_str("\n## Options for every command\n\n");
    for (o, what) in CLI_GLOBAL {
        out.push_str(&format!("- `{o}`: {what}\n"));
    }
    out.push('\n');
    out.push_str(CLI_EXITS);
    out.push('\n');
    out
}

/// `mirqab completions SHELL`. Each script finds the command as the first
/// word that is not a flag or a flag's argument (`mirqab --socket /x load`),
/// completes a flag's argument from its usage line, and otherwise offers the
/// command's positionals and flags.
pub fn cli_completions(shell: &str) -> Option<String> {
    let names = CLI_COMMANDS
        .iter()
        .map(|c| c.name)
        .collect::<Vec<_>>()
        .join(" ");
    let globals = global_words();
    let with_args = flags_with_args();
    let arg_flags = with_args
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join("|");
    Some(match shell {
        "bash" => {
            let mut args = String::new();
            for (flag, arg) in &with_args {
                let reply = match arg {
                    CliArg::Words(w) => {
                        format!(
                            "COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))",
                            w.replace('|', " ")
                        )
                    }
                    CliArg::File => {
                        "compopt -o filenames 2>/dev/null; COMPREPLY=($(compgen -f -- \"$cur\"))"
                            .into()
                    }
                    CliArg::Dir => {
                        "compopt -o filenames 2>/dev/null; COMPREPLY=($(compgen -d -- \"$cur\"))"
                            .into()
                    }
                    CliArg::Free => "COMPREPLY=()".into(),
                };
                args.push_str(&format!("        {flag}) {reply}; return ;;\n"));
            }
            let mut cases = String::new();
            for c in CLI_COMMANDS {
                cases.push_str(&format!(
                    "        {}) opts=\"{}\" ;;\n",
                    c.name,
                    command_words(c)
                ));
            }
            format!(
                "# mirqab bash completion (mirqab completions bash)\n\
                 _mirqab() {{\n    \
                 local cur=${{COMP_WORDS[COMP_CWORD]}} prev=${{COMP_WORDS[COMP_CWORD-1]}} cmd= opts i\n    \
                 for ((i = 1; i < COMP_CWORD; i++)); do\n        \
                 case ${{COMP_WORDS[i]}} in\n            \
                 {arg_flags}) ((i++)) ;;\n            \
                 -*) ;;\n            \
                 *) cmd=${{COMP_WORDS[i]}}; break ;;\n        \
                 esac\n    \
                 done\n    \
                 case $prev in\n{args}    esac\n    \
                 case $cmd in\n        \
                 '') opts=\"{names} {globals}\" ;;\n{cases}        \
                 *) opts=\"{globals}\" ;;\n    \
                 esac\n    \
                 COMPREPLY=($(compgen -W \"$opts\" -- \"$cur\"))\n}}\n\
                 complete -F _mirqab mirqab\n"
            )
        }
        "zsh" => {
            let mut args = String::new();
            for (flag, arg) in &with_args {
                let reply = match arg {
                    CliArg::Words(w) => format!("compadd -- {}", w.replace('|', " ")),
                    CliArg::File => "_files".into(),
                    CliArg::Dir => "_files -/".into(),
                    CliArg::Free => ":".into(),
                };
                args.push_str(&format!("  ({flag}) {reply}; return ;;\n"));
            }
            let mut cases = String::new();
            for c in CLI_COMMANDS {
                cases.push_str(&format!(
                    "  ({}) compadd -- {} ;;\n",
                    c.name,
                    command_words(c)
                ));
            }
            format!(
                "#compdef mirqab\n# mirqab zsh completion (mirqab completions zsh)\n\
                 local cmd= i\n\
                 for (( i = 2; i < CURRENT; i++ )); do\n  \
                 case $words[i] in\n    \
                 ({arg_flags}) (( i++ )) ;;\n    \
                 (-*) ;;\n    \
                 (*) cmd=$words[i]; break ;;\n  \
                 esac\n\
                 done\n\
                 case $words[CURRENT-1] in\n{args}esac\n\
                 case $cmd in\n  \
                 ('') compadd -- {names} {globals} ;;\n{cases}  \
                 (*) compadd -- {globals} ;;\n\
                 esac\n"
            )
        }
        "fish" => {
            let skip = with_args
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>()
                .join(" ");
            let mut out = format!(
                "# mirqab fish completion (mirqab completions fish)\n\
                 function __mirqab_command\n    \
                 set -l skip 0\n    \
                 for w in (commandline -opc)[2..-1]\n        \
                 if test $skip = 1\n            set skip 0\n            continue\n        end\n        \
                 switch $w\n            \
                 case {skip}\n                set skip 1\n            \
                 case '-*'\n            \
                 case '*'\n                echo $w\n                return\n        \
                 end\n    \
                 end\n\
                 end\n\
                 function __mirqab_on\n    \
                 test \"$(__mirqab_command)\" = \"$argv[1]\"\n\
                 end\n\
                 complete -c mirqab -f\n"
            );
            for c in CLI_COMMANDS {
                out.push_str(&format!(
                    "complete -c mirqab -n \"__mirqab_on ''\" -a {} -d '{}'\n",
                    c.name,
                    fish_quote(c.usage[0].1)
                ));
                let positionals = cli_positionals(c);
                if !positionals.is_empty() {
                    out.push_str(&format!(
                        "complete -c mirqab -n '__mirqab_on {}' -a '{}'\n",
                        c.name,
                        positionals.join(" ")
                    ));
                }
                for f in cli_flags(c) {
                    // `-y` is a short flag: fish names it with -s, not -l.
                    let (kind, name) = match f.name.strip_prefix("--") {
                        Some(long) => ("-l", long),
                        None => ("-s", f.name.trim_start_matches('-')),
                    };
                    out.push_str(&format!(
                        "complete -c mirqab -n '__mirqab_on {}' {kind} {name}{}\n",
                        c.name,
                        fish_arg(f.arg)
                    ));
                }
            }
            for f in cli_global_flags() {
                let short = match f.name {
                    "--help" => "-s h ",
                    "--version" => "-s V ",
                    _ => "",
                };
                out.push_str(&format!(
                    "complete -c mirqab {short}-l {}{}\n",
                    f.name.trim_start_matches("--"),
                    fish_arg(f.arg)
                ));
            }
            out
        }
        _ => return None,
    })
}

// --- The MCP server (plan 005) -------------------------------------------------

pub const MCP_INSTRUCTIONS: &str = "mirqab reports this Linux machine's state. Read tools only: system_load for CPU, load, memory, pressure and the thermal word; explain_slowness when the user says the machine is slow; top_apps for the apps using the most CPU or memory; what_is_process to explain one process by pid. Every tool reads the mirqab daemon; if it is not running, the result says how to start it.";
pub const MCP_TOP_APPS_TITLE: &str = "Top apps";
pub const MCP_TOP_APPS: &str = "Reads only: the apps using the most CPU (or memory) now, from the mirqab daemon. Helper processes are folded into their app (a browser's content processes count as the browser, a terminal's jobs as themselves); other users' processes are one System group, listed last. CPU is percent of one core and may pass 100.";
pub const MCP_WHAT_TITLE: &str = "What is this process";
pub const MCP_WHAT: &str = "Reads only: explains one process by pid, from the mirqab daemon: a verdict in plain words (packaged by which repository, Flatpak, container, not owned by any package, replaced since it started), its parents, user, executable, unit, sandbox, memory and counts of open files. Command lines and file paths are left out, as they can hold secrets.";
pub const MCP_ARG_SORT: &str = "cpu (default), mem or name";
pub const MCP_ARG_LIMIT: &str =
    "how many apps, 1 to 50 (default 10); the System group comes on top of them";
pub const MCP_ARG_PID: &str = "the process id";
pub const MCP_PID_REQUIRED: &str = "pid is required: a process id (a positive whole number)";

pub fn mcp_bad_argument(name: &str, expected: &str) -> String {
    format!("invalid argument {name}: {expected}")
}
pub const MCP_SYSTEM_LOAD_TITLE: &str = "System load";
pub const MCP_SYSTEM_LOAD: &str = "Reads only: the machine's CPU use (user, system, I/O wait), load averages, memory and swap, pressure stall percentages and the thermal word (cool, warm, hot, critical), from the mirqab daemon.";
pub const MCP_SLOWNESS_TITLE: &str = "Explain slowness";
pub const MCP_SLOWNESS: &str = "Reads only: explains why the machine feels slow. For now it returns the current load with a note; the rules that name a cause arrive with mirqab's Trends tab.";
pub const MCP_SLOWNESS_LATER: &str = "mirqab cannot name a cause yet: its rules for slowness arrive with the Trends tab (plan 018). The current load:";

pub fn mcp_unknown_tool(name: &str) -> String {
    format!("Unknown tool: {name}")
}

pub fn mcp_unsupported(version: &str) -> String {
    format!("Unsupported protocol version: {version}")
}

pub const MCP_BAD_REQUEST: &str = "Invalid request";
pub const MCP_PARSE_ERROR: &str = "Parse error";
pub const MCP_TOO_LONG: &str = "Request too long";
pub const MCP_NO_CAPABILITIES: &str =
    "_meta must carry io.modelcontextprotocol/clientCapabilities as an object";

pub fn mcp_unknown_method(method: &str) -> String {
    format!("Method not found: {method}")
}

pub const MCP_ARGS_NOT_OBJECT: &str = "arguments must be an object";

// --- mcp-config and confirmation (plan 005) ---------------------------------

pub const MCP_CLIENTS: &[&str] = &["claude", "codex", "gemini", "opencode", "cursor", "json"];

pub fn mcp_bad_client(client: &str) -> String {
    format!(
        "mirqab: mcp-config knows claude, codex, gemini, opencode, cursor and json, not {client}"
    )
}

pub fn mcp_config_run(cmd: &str) -> String {
    format!("Register mirqab's MCP server with:\n  {cmd}\n")
}

pub fn mcp_config_json(path: &str, json: &str) -> String {
    format!("Add this to {path} (merge it with what is there):\n{json}\n")
}

/// Where a client configured by file keeps its MCP servers.
pub fn mcp_config_path(client: &str) -> &'static str {
    match client {
        "cursor" => "~/.cursor/mcp.json",
        _ => "your client's MCP configuration",
    }
}

pub const MCP_CONFIG_PRINT_ONLY: &str = "mirqab: --write needs a client with its own command (claude, codex, gemini, opencode); edit the file yourself";

pub const CONFIRM_WILL: &str = "This will run:";
pub const CONFIRM_PROMPT: &str = "Go ahead? [y/N] ";
pub const CONFIRM_DECLINED: &str = "Not done.";
pub const CONFIRM_NEEDS_TERMINAL: &str =
    "Not done: no terminal to ask in. Run it in a terminal, or pass -y.";

pub fn mcp_config_failed(why: &str) -> String {
    format!("mirqab: the client's command failed: {why}")
}

/// Text for a fish single-quoted string: a backslash first, then a quote.
fn fish_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn fish_arg(arg: Option<CliArg>) -> String {
    // `-x` is `-r -f`: a plain `-r` also offers file names after the flag
    // (`mirqab bar --format <TAB>` listed the directory).
    match arg {
        None => String::new(),
        Some(CliArg::Words(w)) => format!(" -x -a '{}'", w.replace('|', " ")),
        Some(CliArg::File) => " -r -F".into(),
        Some(CliArg::Dir) => " -x -a '(__fish_complete_directories)'".into(),
        Some(CliArg::Free) => " -x".into(),
    }
}

pub fn no_such_series(key: &str) -> String {
    format!("mirqab: history keeps no series named {key}")
}

pub fn output_failed(why: &str) -> String {
    format!("mirqab: could not write the output: {why}")
}

pub fn bad_shell(shell: &str) -> String {
    format!("mirqab: completions are for bash, zsh or fish, not {shell}")
}

pub fn unknown_command(command: &str) -> String {
    format!("mirqab: unknown command '{command}' (see mirqab --help)")
}

pub fn unknown_option(option: &str) -> String {
    format!("mirqab: unknown option '{option}' (see mirqab --help)")
}

pub fn missing_value(option: &str) -> String {
    format!("mirqab: {option} needs a value")
}

pub fn no_daemon(socket: &str, why: &str) -> String {
    format!(
        "mirqab: no daemon at {socket} ({why}); start it with: systemctl --user start mirqab.socket"
    )
}

pub fn daemon_error(code: &str, message: &str) -> String {
    format!("mirqab: {code}: {message}")
}

pub fn no_runtime_dir() -> &'static str {
    "mirqab: XDG_RUNTIME_DIR is not set; the daemon needs a private runtime directory"
}

pub fn runtime_dir_not_ours(dir: &str) -> String {
    format!("mirqab: {dir} is not owned by this user; refusing to put the socket there")
}

pub fn already_running(socket: &str) -> String {
    format!("mirqab: a daemon already listens at {socket}")
}

pub fn history_in_use(path: &str) -> String {
    format!("{path} is in use by another mirqab daemon (on another socket)")
}

pub fn not_a_socket(path: &str) -> String {
    format!("{path} exists and is not a socket; refusing to replace it")
}

pub fn socket_busy(path: &str) -> String {
    format!("{path} is a socket that did not refuse a connection; another daemon may be starting")
}

pub fn ready(socket: &str) -> String {
    format!("ready {socket}")
}

pub fn interface_start_failed(why: &str) -> String {
    format!(
        "could not start mirqab-ui.service ({why}); if the unit cannot see the display, run: systemctl --user import-environment WAYLAND_DISPLAY"
    )
}

pub fn interface_did_not_attach() -> &'static str {
    "mirqab-ui.service started but the interface did not connect within 5 s; see: journalctl --user -u mirqab-ui.service"
}

pub fn config_broken(why: &str) -> String {
    format!("config.json could not be read ({why}); using defaults and leaving the file untouched")
}

pub fn status_lines(
    version: &str,
    socket: &str,
    uptime_sec: u64,
    rss_bytes: u64,
    streams: &[(String, u32, Option<u64>)],
    notes: &[String],
) -> String {
    let mut out = format!(
        "mirqab {version}\nsocket      {socket}\nuptime      {uptime_sec} s\ndaemon      {} KiB resident\n",
        rss_bytes / 1024
    );
    let total: u32 = streams.iter().map(|(_, n, _)| n).sum();
    out.push_str(&format!("subscribers {total}\n"));
    for (name, n, interval) in streams {
        match interval {
            Some(ms) => out.push_str(&format!("  {name:<10} {n} every {ms} ms\n")),
            None => out.push_str(&format!("  {name:<10} {n}\n")),
        }
    }
    for note in notes {
        out.push_str(note);
        out.push('\n');
    }
    out
}

// --- The interface (plan 001 phase D) -------------------------------------

/// The value shown where a number is missing (no sample yet, or none
/// possible on this machine).
pub const NO_VALUE: &str = "\u{2014}";

/// Byte units, SI (1 kB = 1000 B) as GNOME shows sizes.
pub const BYTE_UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
pub const PER_SECOND: &str = "/s";
pub const PERCENT: &str = "%";
pub const DEGREES_C: &str = "\u{b0}C";
pub const DEGREES_F: &str = "\u{b0}F";

/// A duration of whole units: `45s`, `12m`, `5h 12m`, `3d 4h`.
pub fn duration_parts(days: u64, hours: u64, minutes: u64, seconds: u64) -> String {
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
}

/// A tab's name, on its sidebar tile and as its page title.
pub fn tab_label(id: &str) -> Option<&'static str> {
    Some(match id {
        "home" => "Home",
        "apps" => "Apps",
        "system" => "System",
        "fans" => "Fans",
        "disk" => "Disk",
        "network" => "Network",
        "devices" => "Devices",
        "battery" => "Battery",
        "volume" => "Volume",
        "ports" => "Ports",
        "agents" => "Agents",
        "awake" => "Awake",
        "time" => "Time",
        "weather" => "Weather",
        "trends" => "Trends",
        "support" => "Support",
        "settings" => "Settings",
        _ => return None,
    })
}

/// The placeholder on a tab whose plan has not landed yet.
pub fn placeholder_title(tab: &str) -> String {
    format!("{} is not built yet", tab_label(tab).unwrap_or(tab))
}

pub fn placeholder_body(plan: u32) -> String {
    format!("Plan {plan:03} fills this tab.")
}

/// The footer's cost line: daemon and interface together.
pub fn footer_cost(cpu: &str, memory: &str) -> String {
    format!("self {cpu} \u{b7} {memory}")
}

/// The footer's tooltip: the two processes apart.
pub fn footer_cost_detail(daemon: (&str, &str), ui: Option<(&str, &str)>) -> String {
    let mut out = format!("daemon {} \u{b7} {}", daemon.0, daemon.1);
    if let Some((cpu, mem)) = ui {
        out.push_str(&format!("\ninterface {cpu} \u{b7} {mem}"));
    }
    out
}

/// The machine line in the footer: CPU model and uptime.
pub fn machine_line(cpu_model: Option<&str>, uptime: Option<&str>) -> String {
    match (cpu_model, uptime) {
        (Some(c), Some(u)) => format!("{c} \u{b7} up {u}"),
        (Some(c), None) => c.to_string(),
        (None, Some(u)) => format!("up {u}"),
        (None, None) => String::new(),
    }
}

pub const DISCONNECTED: &str = "Not connected to the daemon";
pub fn reconnecting(seconds: u64) -> String {
    format!("Not connected to the daemon; trying again in {seconds} s")
}
pub const POPOVER_UNSUPPORTED: &str =
    "This compositor has no layer shell, so mirqab opens as a window";

pub const THEME_FILE: &str = "theme.json";
pub const FOLLOWED_THEME_FILE: &str = "the followed theme file";

pub fn theme_unreadable(path: &str, why: &str) -> String {
    format!("{path} could not be read ({why}); it is ignored")
}

pub fn theme_not_object(source: &str) -> String {
    format!("{source} is not a JSON object; it is ignored")
}

pub fn theme_unknown_token(source: &str, name: &str) -> String {
    format!("{source}: unknown token '{name}'")
}

pub fn theme_invalid_value(source: &str, name: &str, value: &str) -> String {
    format!("{source}: '{name}' has an invalid value {value}")
}

// --- Window chrome and Settings (plan 001 phase D, part 2) ----------------

pub const PINNED: &str = "Keep open";
pub const MORE: &str = "More";
pub const HEALTH: &str = "Health";
pub const OPEN_AS_WINDOW: &str = "Open as a window";
pub const OPEN_AS_POPOVER: &str = "Open as a popover";
pub const STOP_DAEMON: &str = "Stop the daemon";
pub const CLOSE: &str = "Close";
pub const RANGE: &str = "Range";
pub const PIN_TO_HOME: &str = "Pin to Home";
pub const UNPIN_FROM_HOME: &str = "Unpin from Home";
pub const DRAG_TO_REORDER: &str = "Drag to reorder (Alt+Up, Alt+Down)";
pub const HOME_EMPTY: &str = "Nothing is pinned. Pin a card from its tab to see it here.";
pub const HOME_GO_SYSTEM: &str = "Open System";
pub const CANCEL: &str = "Cancel";
pub const CONFIRM: &str = "Go ahead";
pub const SIDEBAR: &str = "Tabs";

/// A range picker's button. The range ids read as labels in English; a
/// translation maps them here.
#[allow(clippy::needless_match)]
pub fn range_label(range: &str) -> &str {
    match range {
        "2m" => "2m",
        "1h" => "1h",
        "24h" => "24h",
        "7d" => "7d",
        "30d" => "30d",
        other => other,
    }
}

/// A graph's accessible description: its series and latest value.
pub fn graph_summary(series: &str, latest: Option<&str>) -> String {
    match latest {
        Some(v) => format!("{series}, latest {v}"),
        None => format!("{series}, no data yet"),
    }
}

pub const SETTINGS_WINDOW: &str = "Window";
pub const SETTINGS_OPENS_AS: &str = "Opens as";
pub const SETTINGS_POPOVER: &str = "Popover";
pub const SETTINGS_WINDOW_MODE: &str = "Window";
pub const SETTINGS_CORNER: &str = "Corner";
pub const SETTINGS_APPEARANCE: &str = "Appearance";
pub const SETTINGS_COLOURS: &str = "Colours";
pub const SETTINGS_FOLLOW: &str = "Follow a theme file";
pub const SETTINGS_UNITS: &str = "Units";
pub const SETTINGS_TEMPERATURE: &str = "Temperature";
pub const SETTINGS_CLOCK: &str = "Clock";
pub const SETTINGS_LIVE: &str = "Live graphs";
pub const SETTINGS_LIVE_INTERVAL: &str = "Update every (ms)";
pub const SETTINGS_LIVE_HINT: &str =
    "How often a graph on screen updates. Nothing else samples this often.";
pub const SETTINGS_TABS: &str = "Tabs";
pub const SETTINGS_DAEMON: &str = "Daemon";
pub const SETTINGS_VERSION: &str = "Version";
pub const SETTINGS_SOCKET: &str = "Socket";
pub const SETTINGS_HISTORY: &str = "History";
pub const SETTINGS_HELPER: &str = "Root helper";
pub const SETTINGS_HELPER_NONE: &str = "Not installed";
pub const HISTORY_OFF: &str = "Off";

pub fn anchor_label(anchor: &str) -> &str {
    match anchor {
        "top-right" => "Top right",
        "top" => "Top",
        "top-left" => "Top left",
        "bottom-right" => "Bottom right",
        "bottom" => "Bottom",
        "bottom-left" => "Bottom left",
        other => other,
    }
}

pub fn scheme_label(scheme: &str) -> &str {
    match scheme {
        "dark" => "Dark",
        "light" => "Light",
        "system" => "Follow the desktop",
        other => other,
    }
}

pub fn temperature_label(unit: &str) -> &str {
    match unit {
        "c" => "Celsius",
        "f" => "Fahrenheit",
        other => other,
    }
}

pub fn clock_label(clock: &str) -> &str {
    match clock {
        "auto" => "Automatic",
        "12" => "12-hour",
        "24" => "24-hour",
        other => other,
    }
}

pub fn setting_failed(why: &str) -> String {
    format!("Could not save the setting: {why}")
}

pub fn selftest_no_event(tab: &str) -> String {
    format!("mirqab-ui: --selftest {tab}: no event from the daemon within 5 s")
}

pub fn ui_unknown_option(option: &str) -> String {
    format!("mirqab-ui: unknown option '{option}'")
}

pub const UI_USAGE: &str = "\
usage: mirqab-ui [--socket PATH] [--window] [--tab TAB] [--background]
       mirqab-ui --selftest TAB

The window. It is started by `mirqab open` through mirqab-ui.service and
quits 60 s after the window was last hidden.
  --socket PATH   the daemon's socket
  --window        open as a normal window even if the config asks for a popover
  --tab TAB       open on TAB
  --background    start without showing the window (the unit does this)
  --selftest TAB  build TAB in a window, print its widget tree as JSON, exit
";

// --- System tab (plan 002) ------------------------------------------------

/// A sensor kind as a word: `CPU`, `Disk`.
pub fn temp_kind(kind: &str) -> &'static str {
    match kind {
        "cpu" => "CPU",
        "gpu" => "GPU",
        "disk" => "Disk",
        "memory" => "Memory",
        _ => "Sensor",
    }
}

/// A sensor's name when its driver gives no label: `CPU 1`, `Disk 2`.
pub fn temp_label(kind: &str, n: usize) -> String {
    format!("{} {n}", temp_kind(kind))
}

pub fn mhz(v: u32) -> String {
    format!("{v} MHz")
}

/// A GPU's name from its vendor and DRM card: `AMD GPU (card1)`.
pub fn gpu_name(vendor: &str, card: &str) -> String {
    let v = match vendor {
        "amd" => "AMD GPU",
        "intel" => "Intel GPU",
        "nvidia" => "NVIDIA GPU",
        _ => "GPU",
    };
    format!("{v} ({card})")
}

/// `mirqab load`, one line per part; each argument is already formatted.
pub fn load_line(name: &str, value: &str) -> String {
    format!("{name:<9} {value}")
}
pub const LOAD_CPU: &str = "cpu";
pub const LOAD_LOAD: &str = "load";
pub const LOAD_MEMORY: &str = "memory";
pub const LOAD_SWAP: &str = "swap";
pub const LOAD_PRESSURE: &str = "pressure";
pub const LOAD_THERMAL: &str = "thermal";

pub fn load_cpu(total: &str, user: &str, system: &str, iowait: &str) -> String {
    format!("{total} (user {user}, system {system}, iowait {iowait})")
}

pub fn load_avgs(one: f64, five: f64, fifteen: f64, running: u32, tasks: u32) -> String {
    format!("{one:.2} {five:.2} {fifteen:.2} ({running} running of {tasks})")
}

pub fn load_memory(used: &str, total: &str, available: &str) -> String {
    format!("{used} used of {total}, {available} available")
}

pub fn load_swap(used: &str, total: &str) -> String {
    format!("{used} of {total}")
}

pub fn load_pressure(cpu: &str, memory: &str, io: &str) -> String {
    format!("cpu {cpu}  memory {memory}  io {io}  (some, last minute)")
}

/// The thermal word as people read it.
pub fn thermal_word(state: &str, throttling: bool) -> String {
    let w = match state {
        "cool" => "Cool",
        "warm" => "Warm",
        "hot" => "Hot",
        "critical" => "Critical",
        other => other,
    };
    if throttling {
        format!("{w}, throttling")
    } else {
        w.to_string()
    }
}

/// `mirqab sensors`: one temperature.
pub fn sensor_line(kind: &str, label: &str, temp: &str, critical: Option<&str>) -> String {
    match critical {
        Some(c) => format!("{kind:<7} {label:<20} {temp:>7}  critical {c}"),
        None => format!("{kind:<7} {label:<20} {temp:>7}"),
    }
}

/// `mirqab sensors`: one GPU; parts already formatted, absent ones skipped.
pub fn gpu_line(name: &str, parts: &[String]) -> String {
    format!("{:<7} {name:<20} {}", "GPU", parts.join("  "))
}

pub fn gpu_suspended(name: &str) -> String {
    format!("{:<7} {name:<20} suspended", "GPU")
}

pub fn busy(percent: &str) -> String {
    format!("{percent} busy")
}

/// `mirqab sensors`' power line. The helper hint only where RAPL exists
/// but needs root (review of PR #7: it showed on machines with no RAPL).
pub fn power_line(package: Option<&str>, battery: Option<&str>, needs_helper: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    match package {
        Some(w) => parts.push(format!("package {w}")),
        None if needs_helper => parts.push(PACKAGE_POWER_NEEDS_HELPER.to_string()),
        None => {}
    }
    parts.extend(battery.map(|b| format!("battery {b}")));
    if parts.is_empty() {
        parts.push(NO_VALUE.to_string());
    }
    format!("{:<7} {}", "power", parts.join(", "))
}

// --- `mirqab bar` (plan 004) -------------------------------------------------

/// The items `mirqab bar` serves, with a line each for `--list`.
pub const BAR_ITEMS: &[(&str, &str)] = &[
    ("cpu", "CPU busy percent"),
    ("mem", "memory used, percent of total"),
    (
        "temp",
        "the hottest CPU temperature, classed by the thermal word",
    ),
    ("gpu", "the busiest GPU's percent"),
    ("load", "the one-minute load average"),
    ("combo", "several items in one line (config bar.combo)"),
];

pub fn bar_unknown_item(item: &str) -> String {
    format!("unknown bar item: {item} (mirqab bar --list names them)")
}

pub fn bar_bad_interval(v: &str) -> String {
    format!("--interval takes whole seconds, not {v}")
}

pub fn bar_bad_format(v: &str) -> String {
    format!("--format is waybar, plain or json, not {v}")
}

pub const BAR_NEEDS_ITEM: &str = "mirqab bar needs an item; mirqab bar --list names them";

/// Bar texts: a whole percent, a temperature in degrees, a load average.
pub fn bar_percent(p: f64) -> String {
    format!("{}%", p.round() as i64)
}

pub fn bar_degrees(celsius: f64) -> String {
    format!("{}\u{b0}", celsius.round() as i64)
}

pub fn bar_load(one: f64) -> String {
    format!("{one:.1}")
}

pub fn bar_tip_cpu(busy: &str, user: &str, system: &str, iowait: &str) -> String {
    format!("CPU {busy} (user {user}, system {system}, iowait {iowait})")
}

pub fn bar_tip_mem(used: &str, total: &str, available: &str) -> String {
    format!("Memory {used} of {total}, {available} available")
}

pub fn bar_tip_temp(label: &str, temp: &str, word: &str) -> String {
    format!("{label} {temp} ({word})")
}

pub fn bar_tip_gpu(name: &str, busy: &str) -> String {
    format!("{name} {busy}")
}

pub fn bar_tip_load(one: f64, five: f64, fifteen: f64) -> String {
    format!("Load {one:.2} {five:.2} {fifteen:.2}")
}

// --- The tray (plan 004) -----------------------------------------------------

/// The tray's icon name in the icon theme: `dist/icons/mirqab.svg`, two
/// tones, because some bars draw tray icons without recolouring them
/// (plan 021).
pub const TRAY_ICON: &str = "mirqab";
pub const TRAY_OPEN: &str = "Open";
pub const TRAY_QUIT: &str = "Quit daemon";

pub fn tray_tooltip(cpu: &str, mem: &str, temp: &str) -> String {
    format!("CPU {cpu} \u{b7} Mem {mem} \u{b7} {temp}")
}

pub fn tray_action_failed(what: &str, why: &str) -> String {
    format!("tray: {what} failed ({why})")
}

pub fn tray_failed(why: &str) -> String {
    format!("tray: not shown ({why})")
}

pub const PACKAGE_POWER_NEEDS_HELPER: &str = "package power needs the root helper (Fans tab)";
pub const WATTS: &str = "W";

// --- System page (plan 002, part 2) ---------------------------------------

pub const SYSTEM_OVERVIEW: &str = "Overview";
pub const SYSTEM_CPU: &str = "CPU";
pub const SYSTEM_CORES: &str = "Cores";
pub const SYSTEM_MEMORY: &str = "Memory";
pub const SYSTEM_PRESSURE: &str = "Pressure";
pub const SYSTEM_IO: &str = "I/O";
pub const SYSTEM_TEMPERATURES: &str = "Temperatures";
pub const SYSTEM_POWER: &str = "Power";
pub const SYSTEM_PRESSURE_HINT: &str =
    "The share of the last minute in which some tasks waited for this resource.";
pub const SYSTEM_USER: &str = "user";
pub const SYSTEM_SYS: &str = "system";
pub const SYSTEM_IOWAIT: &str = "iowait";
pub const SYSTEM_USED: &str = "used";
pub const SYSTEM_CACHE: &str = "cache";
pub const SYSTEM_FREE: &str = "free";
pub const SYSTEM_PACKAGE: &str = "Package";
pub const SYSTEM_BATTERY: &str = "Battery";
pub const SYSTEM_SUSPENDED: &str = "Suspended; not woken to read it";
pub const SYSTEM_GRAPHICS: &str = "Graphics";
pub const SYSTEM_VRAM: &str = "VRAM";
pub const SYSTEM_BUSY: &str = "Busy";

pub fn system_uptime(d: &str) -> String {
    format!("up {d}")
}

pub fn system_load(one: f64, five: f64, fifteen: f64) -> String {
    format!("load {one:.2} {five:.2} {fifteen:.2}")
}

pub fn system_clock(avg_ghz: f64, max_ghz: Option<f64>) -> String {
    match max_ghz {
        Some(m) => format!("{avg_ghz:.1} GHz now, {m:.1} GHz max"),
        None => format!("{avg_ghz:.1} GHz now"),
    }
}

pub fn system_used_of(used: &str, total: &str) -> String {
    format!("{used} of {total}")
}

pub fn system_swap(used: &str, total: &str) -> String {
    format!("Swap {used} of {total}")
}

pub fn system_zram(orig: &str, compr: &str) -> String {
    format!("Compressed swap {orig} in {compr}")
}

pub fn system_paging(page_in: &str, page_out: &str) -> String {
    format!("Paging in {page_in}, out {page_out}")
}

pub fn system_advice(reason: &str) -> &'static str {
    match reason {
        "uptime-30d" => "Up for over a month: a restart is recommended",
        "uptime-7d-swap" => "Swap is more than half full after a week: a restart is recommended",
        "uptime-7d-pressure" => {
            "Memory has been under pressure for an hour: a restart is recommended"
        }
        _ => "A restart is recommended",
    }
}

pub fn core_label(id: u32) -> String {
    format!("{id}")
}

pub fn per_second(v: &str) -> String {
    format!("{v}/s")
}

// --- Apps and the process inspector (plan 006) ------------------------------

/// The group every other user's processes fold into.
pub const APPS_SYSTEM: &str = "System";

/// A container's group: its main process and the short id `docker ps` shows.
pub fn apps_container(name: &str, id: &str) -> String {
    format!("{name} (container {id})")
}

pub const APPS_PID_REQUIRED: &str = "pid is required: a process id";

pub fn apps_no_process(pid: u32) -> String {
    format!("no process {pid}")
}

pub const VERDICT_KERNEL: &str = "A kernel thread: part of Linux itself";
pub const VERDICT_REPLACED: &str =
    "Executable was replaced since it started (an update); restart it";
pub const VERDICT_FLATPAK: &str = "Flatpak app, sandboxed";
pub const VERDICT_CONTAINER: &str = "Runs in a container";
pub const VERDICT_HOME: &str = "Not owned by any package, running from your home directory";
pub const VERDICT_UNOWNED: &str = "Not owned by any package";
pub const VERDICT_UNKNOWN: &str =
    "Package not checked: no package database to ask, or the executable cannot be read";

pub fn verdict_other_user(user: &str) -> String {
    format!("Belongs to {user}; only its name, CPU and memory are shown")
}

pub fn verdict_packaged(name: &str, repo: Option<&str>) -> String {
    match repo {
        Some(r) => format!("Packaged by {r}/{name}"),
        None => format!(
            "Packaged by {name}, which no sync repository provides (built locally or from the AUR)"
        ),
    }
}

/// `mirqab apps`: the header and one row per group; values already
/// formatted.
pub const APPS_HEADER: &str = "NAME                         CPU      MEMORY  PROCESSES";

pub fn apps_row(name: &str, cpu: &str, memory: &str, procs: u32) -> String {
    let name = fit(name, 28);
    format!("{name:<28} {cpu:>6} {memory:>11}  {procs}")
}

/// `mirqab apps --all`: one process under its group.
pub fn apps_process_row(pid: u32, name: &str, cpu: &str, memory: &str) -> String {
    let name = fit(name, 18);
    format!("  {pid:>7} {name:<18} {cpu:>6} {memory:>11}")
}

/// `text` cut to `width` characters, an ellipsis marking the cut, so a long
/// name keeps the columns of a table.
fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut s: String = text.chars().take(width - 1).collect();
    s.push('\u{2026}');
    s
}

pub fn apps_bad_sort(v: &str) -> String {
    format!("mirqab: --sort is cpu, mem or name, not {v}")
}

pub fn bad_pid(v: &str) -> String {
    format!("mirqab: not a process id: {v}")
}

/// `mirqab what`: one labelled line.
pub fn what_line(label: &str, value: &str) -> String {
    format!("{label:<11} {value}")
}

pub const WHAT_PROCESS: &str = "process";
pub const WHAT_PARENTS: &str = "parents";
pub const WHAT_USER: &str = "user";
pub const WHAT_STARTED: &str = "started";
pub const WHAT_CPU_TIME: &str = "cpu time";
pub const WHAT_THREADS: &str = "threads";
pub const WHAT_STATE: &str = "state";
pub const WHAT_EXE: &str = "executable";
pub const WHAT_PACKAGE: &str = "package";
pub const WHAT_ARCH: &str = "arch";
pub const WHAT_UNIT: &str = "unit";
pub const WHAT_GROUP: &str = "group";
pub const WHAT_FLATPAK: &str = "flatpak";
pub const WHAT_CONTAINER: &str = "container";
pub const WHAT_SANDBOX: &str = "sandbox";
pub const WHAT_MEMORY: &str = "memory";
pub const WHAT_FILES: &str = "files";
pub const WHAT_GPU: &str = "gpu";
pub const WHAT_CWD: &str = "directory";
pub const WHAT_COMMAND: &str = "command";

/// A process by name and pid: `browser (2001)`.
pub fn proc_ref(name: &str, pid: u32) -> String {
    format!("{name} ({pid})")
}

pub fn started_ago(duration: &str) -> String {
    format!("{duration} ago")
}

pub fn exe_replaced(path: &str) -> String {
    format!("{path} (replaced since it started)")
}

pub fn package_line(name: &str, repo: Option<&str>) -> String {
    match repo {
        Some(r) => format!("{r}/{name}"),
        None => format!("{name} (in no sync repository)"),
    }
}

pub fn arch_line(machine: &str, bits: u32, foreign: bool) -> String {
    if foreign {
        format!("{machine}, {bits}-bit, emulated (not this machine's type)")
    } else {
        format!("{machine}, {bits}-bit")
    }
}

/// The inspector's sandbox line; each part only when it says something.
pub fn sandbox_line(
    seccomp: Option<&str>,
    no_new_privs: bool,
    caps: &[String],
    ns: &[String],
) -> String {
    let mut parts = vec![format!("seccomp {}", seccomp.unwrap_or(NO_VALUE))];
    if no_new_privs {
        parts.push("no-new-privileges".into());
    }
    if !caps.is_empty() {
        parts.push(format!("capabilities {}", caps.join(" ")));
    }
    if !ns.is_empty() {
        parts.push(format!("own namespaces {}", ns.join(" ")));
    }
    parts.join(", ")
}

pub fn memory_line(rss: &str, pss: Option<&str>, swap: Option<&str>) -> String {
    let mut s = format!("{rss} resident");
    if let Some(p) = pss {
        s.push_str(&format!(", {p} proportional"));
    }
    if let Some(w) = swap {
        s.push_str(&format!(", {w} swapped"));
    }
    s
}

pub fn files_line(count: u32, sockets: u32) -> String {
    format!("{count} open, {sockets} sockets")
}

pub fn gpu_memory_line(vram: Option<&str>, gtt: Option<&str>, cpu: Option<&str>) -> String {
    let mut parts = Vec::new();
    parts.extend(vram.map(|v| format!("{v} VRAM")));
    parts.extend(gtt.map(|v| format!("{v} GTT")));
    parts.extend(cpu.map(|v| format!("{v} system")));
    parts.join(", ")
}

/// A process state letter as a word.
pub fn state_word(state: &str) -> &str {
    match state {
        "R" => "running",
        "S" => "sleeping",
        "D" => "waiting on storage",
        "Z" => "zombie (exited, not yet reaped)",
        "T" | "t" => "stopped",
        "I" => "idle",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_cli_md_is_the_generated_markdown() {
        // Regenerate with: mirqab help --markdown > docs/cli.md
        assert_eq!(include_str!("../../../docs/cli.md"), cli_markdown());
    }

    #[test]
    fn completions_name_every_command_and_flag() {
        for shell in ["bash", "zsh", "fish"] {
            let script = cli_completions(shell).unwrap();
            for c in CLI_COMMANDS {
                assert!(script.contains(c.name), "{shell}: {}", c.name);
                for f in cli_flags(c) {
                    assert!(
                        script.contains(f.name.trim_start_matches('-')),
                        "{shell}: {}",
                        f.name
                    );
                }
            }
        }
        assert!(cli_completions("tcsh").is_none());
    }

    #[test]
    fn flags_and_their_arguments_come_from_the_usage_lines() {
        let flags = cli_flags(cli_command("history").unwrap());
        assert_eq!(
            flags,
            [
                CliFlag {
                    name: "--range",
                    arg: Some(CliArg::Words("1h|24h|7d|30d"))
                },
                CliFlag {
                    name: "--json",
                    arg: None
                },
                CliFlag {
                    name: "--list",
                    arg: None
                },
            ]
        );
        let daemon = cli_flags(cli_command("daemon").unwrap());
        assert_eq!(daemon[0].arg, Some(CliArg::Dir));
        assert_eq!(daemon[1].arg, Some(CliArg::File));
        let bar = cli_flags(cli_command("bar").unwrap());
        assert!(bar.contains(&CliFlag {
            name: "--interval",
            arg: Some(CliArg::Free)
        }));
        for c in ["open", "toggle", "close"] {
            assert!(
                cli_flags(cli_command(c).unwrap())
                    .iter()
                    .any(|f| f.name == "--dry-run"),
                "{c} takes --dry-run"
            );
        }
        assert_eq!(
            cli_positionals(cli_command("completions").unwrap()),
            ["bash", "zsh", "fish"]
        );
        assert!(cli_positionals(cli_command("help").unwrap()).contains(&"sensors"));
        assert!(cli_positionals(cli_command("bar").unwrap()).contains(&BAR_ITEMS[0].0));
    }

    #[test]
    fn one_flag_completes_the_same_way_for_every_command() {
        let mut seen: Vec<CliFlag> = cli_global_flags().to_vec();
        for f in CLI_COMMANDS.iter().flat_map(cli_flags) {
            if let Some(other) = seen.iter().find(|s| s.name == f.name) {
                assert_eq!(other.arg, f.arg, "{} differs between commands", f.name);
            } else {
                seen.push(f);
            }
        }
    }

    #[test]
    fn the_usage_fits_and_every_line_has_a_summary() {
        let usage = cli_usage();
        for line in usage.lines() {
            assert!(line.len() <= 100, "too wide: {line}");
        }
        for c in CLI_COMMANDS {
            for (u, summary) in c.usage {
                assert!(!summary.is_empty(), "{u} has no summary");
                assert!(usage.contains(summary), "{summary} missing");
            }
        }
    }

    #[test]
    fn every_command_in_the_usage_has_help() {
        // main.rs dispatching each of these is tests/test-cli.sh's job.
        for c in CLI_COMMANDS {
            assert!(
                cli_usage().contains(&format!("  {} ", c.name))
                    || cli_usage().contains(&format!("  {}\n", c.name)),
                "{} missing from usage",
                c.name
            );
            assert!(command_help(c.name).is_some(), "{} has no help", c.name);
        }
        assert!(command_help("nope").is_none());
    }

    #[test]
    fn status_counts_subscribers() {
        let s = status_lines(
            "0.1.0",
            "/tmp/s",
            5,
            4096,
            &[("self".into(), 2, Some(2000)), ("ui".into(), 1, None)],
            &[],
        );
        assert!(s.contains("subscribers 3"));
        assert!(s.contains("self       2 every 2000 ms"));
    }
}
