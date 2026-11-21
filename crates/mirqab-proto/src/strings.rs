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
        help: "Prints the version, the socket, each stream's subscribers and interval, what the daemon costs, and the focus source the Apps page's \"idle but heavy\" tag uses (Hyprland, only while the page is open).",
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
        help: "One row per app: CPU (percent of one core, over the last seconds), memory and how many processes, heaviest first. A browser's helpers count as the browser and a terminal's jobs as themselves; other users' processes are one System group, last. --sort orders by cpu (default), mem or name. --all lists every process under its app. On Hyprland, while the Apps page is open, an app that used at least 20 % of a core for five minutes with none of its windows in focus is tagged in an IDLE column, shown only when some row is tagged.",
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
        name: "quit",
        usage: &[(
            "quit PID|NAME [-y] [--force] [--dry-run] [--json]",
            "quit an app or a process, after asking",
        )],
        help: "Asks a process to quit (SIGTERM), or with --force ends it (SIGKILL). A number is a pid; anything else is an app's name as mirqab apps shows it, matched whole and ignoring case, and quits every process of that app, its main process first. A user service is stopped with systemctl --user, so it does not restart; the list names the other services the stop takes with it, and warns before the sound server or a desktop portal is stopped. The stop is refused when one of those services holds a process mirqab never signals, and at the confirmation when a service that was not listed would now stop too; when systemd cannot say what depends on a service, the list says so. It prints what it will do and asks; -y goes ahead without asking; without a terminal and without -y it does nothing and exits 3; --dry-run prints the list and stops. With --json the list goes to stderr and stdout holds the daemon's answer. It exits 1 when anything failed, or when nothing was done because everything was left alone; a process that had already exited counts as done. Never stopped or signalled: pid 1, another user's processes, kernel threads, anything outside your user's session, the daemon, the session's compositor, your user manager, the session bus (also any process named dbus-daemon, dbus-broker or dbus-broker-launch), and the program asking and its parents. A rootless container is not quit; stop it with its runtime. When the action could not be written to mirqab's history, it is still done and a warning says so.",
    },
    CliCommand {
        name: "ports",
        usage: &[(
            "ports [--udp] [--json]",
            "what is listening, and whether the network can reach it",
        )],
        help: "Lists every listening TCP port: the address it is bound to, how far it reaches (local: only this machine; all: every interface; lan, bridge, vpn, link or other: that network; unknown: an address on no interface, treated as reachable), the process holding it and how long it has run. A port of root's or another user's shows its owner and no process: mirqab cannot see which process holds it. A line after the table names each port of yours that the network can reach. mirqab reads the bind address only and never your firewall, so a port the network can reach may still be blocked by a firewall. --udp adds bound UDP sockets.",
    },
    CliCommand {
        name: "port",
        usage: &[(
            "port PORT [--proto tcp|udp] [--json]",
            "everything about one port",
        )],
        help: "Shows everything mirqab knows about what listens on PORT (TCP unless --proto udp): one block per listener with its reach in words, the process, what it serves and how long it has run, and the URL to open it. It exits 1 when nothing listens there.",
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
            // A list of words, not of placeholders (`PID|NAME`).
            b if b.contains('|') && !b.starts_with('-') && !b.contains(char::is_uppercase) => {
                out.extend(b.split('|'))
            }
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
pub const MCP_TOP_APPS: &str = "Reads only: the apps using the most CPU (or memory) now, from the mirqab daemon. Helper processes are folded into their app (a browser's content processes count as the browser, a terminal's jobs as themselves); other users' processes are one System group, listed last. CPU is percent of one core and may pass 100. A group marked idleHeavy used at least 20 % of a core with none of its windows in focus for five minutes; the mark appears only while the Apps page has been open five minutes on Hyprland.";
pub const MCP_WHAT_TITLE: &str = "What is this process";
pub const MCP_WHAT: &str = "Reads only: explains one process by pid, from the mirqab daemon: a verdict in plain words (packaged by which repository, Flatpak, container, not owned by any package, replaced since it started), its parents, user, executable, unit, sandbox, memory and counts of open files. Command lines, the working directory and open files' paths are left out, as they can hold secrets.";
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

/// A request the daemon took but did not answer in time: it is running,
/// so this is not "no daemon", and an action may still have run.
pub fn daemon_no_answer(secs: u64) -> String {
    format!(
        "mirqab: the daemon did not answer within {secs} s; it is running, and a requested action may still have run"
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

/// `systemctl --user start` itself failed: the unit is missing or broken,
/// or there is no user manager. A unit that cannot see the display still
/// starts (it is Type=simple), so that hint is on the next message.
pub fn interface_start_failed(why: &str) -> String {
    format!(
        "could not start mirqab-ui.service ({why}); is the unit installed in ~/.config/systemd/user? If you just copied it, run: systemctl --user daemon-reload"
    )
}

/// The unit started and nothing attached: most often it cannot see the
/// display, since GTK opens it after systemd reported the start.
pub fn interface_did_not_attach() -> &'static str {
    "mirqab-ui.service started but the interface did not connect within 5 s; if journalctl --user -u mirqab-ui.service says it cannot open the display, run: systemctl --user import-environment WAYLAND_DISPLAY"
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

// --- The daemon's replies and checks (architecture §5.2) ------------------

pub fn bad_json(why: &str) -> String {
    format!("bad json: {why}")
}

pub fn unknown_method(method: &str) -> String {
    format!("unknown method: {method}")
}

pub const STREAM_REQUIRED: &str = "stream is required";

pub fn unknown_stream(name: &str) -> String {
    format!("unknown stream: {name}")
}

pub const UI_EVENTS_ONLY: &str = "ui has events only";
pub const KEY_REQUIRED: &str = "key is required";
pub const NAME_REQUIRED: &str = "name is required";

pub fn unknown_action(name: &str) -> String {
    format!("unknown action: {name}")
}

pub const HISTORY_IS_OFF: &str = "history is off";
pub const SERIES_REQUIRED: &str = "series is required";

pub fn too_many_series(max: usize) -> String {
    format!("at most {max} series in one query")
}

pub fn unknown_range(name: &str) -> String {
    format!("unknown range: {name}")
}

pub const RANGE_REQUIRED: &str = "range, or from and to, are required";
pub const SPAN_NOT_KEPT: &str = "from and to lie outside the 30 days history keeps";
pub const SPAN_IN_FUTURE: &str = "from and to lie in the future, where history has nothing yet";
pub const HISTORY_ASIDE_TAKEN: &str = "every name to move the damaged history file to is taken";

pub fn history_damaged_again(hours: i64) -> String {
    format!(
        "history was damaged again within {hours} hours of being replaced; it is off and the file is left in place until then"
    )
}

pub fn history_schema_newer(found: &str, ours: &str) -> String {
    format!("history schema {found} is newer than {ours}")
}

pub fn history_series_full(max: usize) -> String {
    format!("history holds at most {max} series")
}

pub fn cannot_start(why: &str) -> String {
    format!("cannot start: {why}")
}

pub const TIMED_OUT: &str = "timed out";

// `config.set`'s checks, one per kind of value.

pub fn setting_one_of(key: &str, allowed: &[&str]) -> String {
    format!("{key} must be one of {}", allowed.join(", "))
}

pub fn setting_range(key: &str, lo: f64, hi: f64) -> String {
    format!("{key} must be a number from {lo} to {hi}")
}

pub fn setting_string_or_null(key: &str) -> String {
    format!("{key} must be a string or null")
}

pub fn setting_strings(key: &str) -> String {
    format!("{key} must be a list of strings")
}

pub fn setting_bool(key: &str) -> String {
    format!("{key} must be true or false")
}

pub fn setting_tab_or_null(key: &str) -> String {
    format!("{key} must be a tab id or null")
}

pub fn setting_interval_ms(key: &str, lo: u64, hi: u64) -> String {
    format!("{key} must be a whole number of milliseconds from {lo} to {hi}")
}

pub fn setting_bar_combo(key: &str) -> String {
    format!("{key} must be a list of bar items other than combo (mirqab bar --list)")
}

pub fn unknown_setting(key: &str) -> String {
    format!("unknown setting: {key}")
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

/// A tile's count badge in words, its accessible description: Ports
/// counts listening ports, Agents the sessions that need the user
/// (architecture §7.1; audit 2026-11-21, L54). Another tab's count is
/// read as the bare number.
pub fn tile_badge(tab: &str, n: u32) -> String {
    match (tab, n) {
        ("ports", 1) => "1 port listening".to_string(),
        ("ports", _) => format!("{n} ports listening"),
        ("agents", 1) => "1 session needs you".to_string(),
        ("agents", _) => format!("{n} sessions need you"),
        _ => n.to_string(),
    }
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
    format!("{} (container {})", plain(name), plain(id))
}

/// `s` safe to print: every control character (C0, DEL, C1) and the
/// bidirectional overrides and isolates (U+202A–202E, U+2066–2069) become
/// U+FFFD. A process names itself, and an escape sequence or a newline in
/// its name would rewrite the terminal or forge rows in `mirqab apps` and
/// in the text an MCP client reads (review of PR #16). JSON needs none of
/// this: serde escapes.
pub fn plain(s: &str) -> std::borrow::Cow<'_, str> {
    let bad =
        |c: char| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    if !s.chars().any(bad) {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(
        s.chars()
            .map(|c| if bad(c) { '\u{fffd}' } else { c })
            .collect(),
    )
}

pub const APPS_PID_REQUIRED: &str = "pid is required: a process id";
/// A `process` subscription or `get` whose params are not
/// `{"pid": N}` with an optional `"start"` (plan 006, part 3).
pub const PROCESS_PARAMS: &str =
    "params are {\"pid\": N} and optionally \"start\": a process id and its start time";

pub fn apps_no_process(pid: u32) -> String {
    format!("no process {pid}")
}

/// A pid asked for with a start time that is not its process's now: the
/// process exited and the pid was given to another (plan 006, part 4,
/// decisions 8 and 9).
pub fn apps_process_gone(pid: u32) -> String {
    format!("pid {pid} is another process now")
}

/// `start` beside a pid that is not a whole number.
pub const APPS_START_NUMBER: &str =
    "start must be a whole number: the process's start time, as the apps stream lists it";

// --- Focus (plan 006, part 4) -----------------------------------------------

/// No Hyprland instance in the runtime directory: no focus source.
pub const FOCUS_NO_HYPRLAND: &str = "no Hyprland instance";
/// Hyprland's answer to `j/activewindow` was not an object, or too long.
pub const FOCUS_BAD_ANSWER: &str = "Hyprland's answer is not a window";

/// `mirqab status`: the focus source, or none.
pub fn status_focus(source: Option<&str>) -> String {
    match source {
        Some(s) => format!("focus       {}", plain(s)),
        None => "focus       none (the Apps page is closed, or no Hyprland)".to_string(),
    }
}

// --- Quit and Force quit (plan 006, part 2) ---------------------------------

pub fn refuse_init(pid: u32) -> String {
    format!("pid {pid} is never signalled")
}

pub fn refuse_daemon(pid: u32) -> String {
    format!("pid {pid} is the mirqab daemon, which does not signal itself")
}

pub fn refuse_kernel(pid: u32) -> String {
    format!("pid {pid} is a kernel thread, part of Linux itself")
}

pub fn refuse_other_user(pid: u32) -> String {
    format!("pid {pid} belongs to another user, or runs with another user's rights")
}

pub fn refuse_outside(pid: u32) -> String {
    format!("pid {pid} is outside your own processes (not under your user slice)")
}

pub fn refuse_user_manager(pid: u32) -> String {
    format!("pid {pid} is your user manager (systemd --user); ending it ends your session")
}

pub fn refuse_compositor(pid: u32) -> String {
    format!("pid {pid} is the session's compositor; ending it ends your session")
}

pub fn refuse_compositor_parent(pid: u32) -> String {
    format!("pid {pid} started the session's compositor")
}

pub fn refuse_caller(pid: u32) -> String {
    format!("pid {pid} is the program asking, or one of its parents")
}

pub fn refuse_unreadable(pid: u32) -> String {
    format!("pid {pid} cannot be read, so it is left alone")
}

/// Why the walk up a process's parents stopped short.
pub fn walk_unreadable(pid: u32) -> String {
    format!("proc/{pid}/stat cannot be read")
}

pub fn walk_loop(pid: u32) -> String {
    format!("the parent links loop at pid {pid}")
}

pub fn walk_too_deep(depth: usize) -> String {
    format!("more than {depth} parents")
}

pub const ACT_NO_CLOCK: &str = "the boot-time clock cannot be read, so no plan is made or used";
pub const QUIT_NOT_RECORDED: &str =
    "mirqab: done, but not recorded: the daemon could not write this action to its history";

pub fn refuse_compositor_unknown(why: &str) -> String {
    format!("the session's compositor cannot be told ({why}), so nothing is signalled")
}

pub const ACT_DRY_RUN_BOOL: &str = "dryRun must be true or false";
pub const QUIT_NO_CONFIRM_ASKED: &str = "it answered without asking for confirmation";
pub const QUIT_NO_PLAN: &str = "its needs-confirm reply holds no plan";

pub fn refuse_tree(why: &str) -> String {
    format!("the process tree could not be read ({why}), so nothing is signalled")
}

pub const REFUSE_NO_CALLER: &str =
    "the program asking cannot be identified, so nothing is signalled";

pub fn refuse_unit_name(unit: &str) -> String {
    format!(
        "the unit name {} is not one mirqab passes to systemctl",
        plain(unit)
    )
}

pub fn refuse_unit_holds(unit: &str, why: &str) -> String {
    format!("{} is not stopped: {why}", plain(unit))
}

pub fn refuse_container(id: &str, runtime: Option<&str>) -> String {
    match runtime {
        Some(r) => format!(
            "container {id} runs processes on another uid, which you cannot signal; stop it with: {r} stop {id}"
        ),
        None => format!(
            "container {id} runs processes on another uid, which you cannot signal; stop it with its container runtime"
        ),
    }
}

/// A process of the session bus (owner, 2026-11-14, plan 006 Q2).
pub fn refuse_session_bus(pid: u32, unit: &str) -> String {
    format!(
        "pid {pid} is the session bus ({}); ending it ends your session",
        plain(unit)
    )
}

/// A session bus found by its command name, in any unit (owner,
/// 2026-11-14, part 4 Q1).
pub fn refuse_session_bus_comm(pid: u32, comm: &str) -> String {
    format!(
        "pid {pid} is a session bus ({}); the programs on it lose it when it ends, and ending your session's bus ends your session",
        plain(comm)
    )
}

/// The session bus's unit, asked to stop.
pub fn refuse_session_bus_unit(unit: &str) -> String {
    format!(
        "{} is the session bus, which is never stopped: ending it ends your session",
        plain(unit)
    )
}

/// A unit whose stop would take down a unit holding a refused process
/// (plan 006, part 3, decision 9).
pub fn refuse_unit_dependent(unit: &str, dependent: &str, why: &str) -> String {
    format!(
        "{} is not stopped: stopping it also stops {}, and {why}",
        plain(unit),
        plain(dependent)
    )
}

/// The warning a stop of the sound server carries (owner, 2026-11-14).
pub fn warn_sound(unit: &str) -> String {
    format!(
        "{} is part of your sound: apps lose sound until something starts it again",
        plain(unit)
    )
}

/// The warning a stop of a desktop portal carries (owner, 2026-11-14).
pub fn warn_portal(unit: &str) -> String {
    format!(
        "{} is a desktop portal: file pickers and screen sharing in apps fail until something starts it again",
        plain(unit)
    )
}

pub fn refuse_nothing_left(first: &str) -> String {
    format!("nothing is left to quit: {first}")
}

pub fn act_gone(pid: u32) -> String {
    format!("pid {pid} has exited")
}

pub const SKIP_LATE: &str = "started after the confirmation";

/// A name the service manager gave as a dependent that mirqab will not pass
/// back to it.
pub fn act_odd_dependent(name: &str) -> String {
    format!(
        "the unit name {} is not one mirqab passes to systemctl",
        plain(name)
    )
}
pub const ACT_NO_PIDFD: &str =
    "this kernel cannot signal through a pidfd (Linux 5.3 or later is needed)";

pub fn act_not_leader(pid: u32) -> String {
    format!("pid {pid} is a thread, not a process")
}

pub fn apps_no_group(id: &str) -> String {
    format!("no app group {}", plain(id))
}

pub const ACT_ARGS: &str =
    "args is {\"pid\": N}, {\"pid\": N, \"start\": S} or {\"group\": \"ID\"}, and nothing else";
pub const ACT_CONFIRM_BOOL: &str = "confirm must be true or false";
pub const ACT_CONFIRM_FALSE: &str = "a plan is redeemed only with confirm: true";
pub const ACT_PLAN_STRING: &str = "plan must be a plan id";
pub const ACT_NEEDS_CONFIRM: &str =
    "nothing was done; to go ahead, ask again with confirm: true and this plan";
pub const PLAN_UNKNOWN: &str = "no such plan: already used, expired or never made";
pub const PLAN_EXPIRED: &str = "the plan expired (after 2 minutes); ask again";
pub const PLAN_OTHER: &str = "the plan was made for another action or target";

/// A confirmation whose unit would now take with it a unit the plan did
/// not show (review of PR #18).
pub fn plan_dependents_changed(unit: &str, new: &[String]) -> String {
    let new: Vec<String> = new.iter().map(|n| plain(n).into_owned()).collect();
    format!(
        "nothing was done: stopping {} would now also stop {}, which the confirmation did not show; ask again",
        plain(unit),
        new.join(", ")
    )
}

/// A confirmation whose unit's dependents were listed in the plan and
/// cannot be told now.
pub fn plan_dependents_unknown(unit: &str) -> String {
    format!(
        "nothing was done: which services stop with {} cannot be told now, and the confirmation listed them; ask again",
        plain(unit)
    )
}

/// A confirmation whose target is now stopped through another unit than
/// the plan showed: a unit where the plan showed signals by pid, another
/// unit, or signals where it showed a unit (audit of plan 006).
pub fn plan_unit_changed(planned: Option<&str>, now: Option<&str>) -> String {
    let how = |u: Option<&str>| match u {
        Some(u) => format!("a stop of {}", plain(u)),
        None => "signals by pid".to_string(),
    };
    format!(
        "nothing was done: the confirmation showed {}, and the target now needs {}; ask again",
        how(planned),
        how(now)
    )
}

/// `mirqab quit`: the line above the list, for Quit and Force quit.
pub const QUIT_WILL: &str = "This will quit:";
pub const FORCE_QUIT_WILL: &str = "This will force quit:";

/// One process `mirqab quit` signals; `via` is the thread id asked for.
pub fn quit_signal_line(signal: &str, pid: u32, name: &str, via: Option<u32>) -> String {
    let name = plain(name);
    match via {
        Some(t) => format!("{signal:<4} {pid:>7}  {name} (asked for its thread {t})"),
        None => format!("{signal:<4} {pid:>7}  {name}"),
    }
}

/// A command `mirqab quit` runs for a service, and how many processes it ends.
pub fn quit_run_line(argv: &str, procs: usize) -> String {
    let what = if procs == 1 { "process" } else { "processes" };
    format!("{} ({procs} {what})", plain(argv))
}

/// A unit a service's stop takes down with it, and how many processes it
/// ends (plan 006, part 3). `deeper` for a unit that is no service, scope
/// or slice (a target), whose own dependents stop too, unlisted (review of
/// PR #18, S3).
pub fn quit_also_stops_line(unit: &str, procs: usize, deeper: bool) -> String {
    let what = if procs == 1 { "process" } else { "processes" };
    let tail = if deeper {
        " and what depends on it"
    } else {
        ""
    };
    format!("  also stops {} ({procs} {what}){tail}", plain(unit))
}

/// When the daemon could not ask which units depend on a service.
pub const QUIT_ALSO_STOPS_UNKNOWN: &str = "  other services that depend on it may stop too";

/// A warning the daemon gave with the plan (the sound server, a portal).
pub fn quit_warning_line(text: &str) -> String {
    format!("warning: {}", plain(text))
}

pub fn quit_skipped_line(pid: u32, name: &str, reason: &str) -> String {
    format!("left alone {pid:>7}  {}: {reason}", plain(name))
}

pub fn quit_done_line(signal: &str, pid: u32, name: &str) -> String {
    format!("sent {signal} to {pid} {}", plain(name))
}

pub fn quit_unit_done_line(argv: &str) -> String {
    format!("ran {}", plain(argv))
}

pub fn quit_gone_line(pid: u32) -> String {
    format!("{pid} had already exited")
}

pub fn quit_failed_line(what: &str, why: &str) -> String {
    format!("failed {}: {}", plain(what), plain(why))
}

pub fn quit_no_app(name: &str) -> String {
    format!(
        "mirqab: no app is named {} (mirqab apps lists them)",
        plain(name)
    )
}

pub fn quit_ambiguous(name: &str, n: usize) -> String {
    format!(
        "mirqab: {n} apps are named {}; quit one by a pid of it:",
        plain(name)
    )
}

pub fn quit_candidate(id: &str, pids: &[u32]) -> String {
    let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
    format!("  {}  pids {}", plain(id), pids.join(" "))
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

/// `mirqab apps` when a row is "idle but heavy" (plan 006, part 4): the
/// header and the rows gain a last column.
pub const APPS_HEADER_IDLE: &str = "NAME                         CPU      MEMORY  PROCESSES  IDLE";

/// A row of `apps_row` with the last column: `idle` when tagged.
pub fn apps_row_idle(row: &str, idle: bool) -> String {
    if idle {
        format!("{row:<55}  idle")
    } else {
        row.to_string()
    }
}

/// `mirqab apps --all`: one process under its group.
pub fn apps_process_row(pid: u32, name: &str, cpu: &str, memory: &str) -> String {
    let name = fit(name, 18);
    format!("  {pid:>7} {name:<18} {cpu:>6} {memory:>11}")
}

/// `text` made `plain` and cut to `width` characters, an ellipsis marking
/// the cut, so a long name keeps the columns of a table.
fn fit(text: &str, width: usize) -> String {
    let text = plain(text);
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut s: String = text.chars().take(width - 1).collect();
    s.push('\u{2026}');
    s
}

/// `mirqab apps`' last line when the daemon's limit cut the list.
pub fn apps_more(n: u32) -> String {
    format!("and {n} more")
}

/// The daemon answered with data this mirqab cannot read: after an
/// upgrade, the running daemon is often the old one.
pub fn bad_daemon_answer(why: &str) -> String {
    format!(
        "mirqab: the daemon's answer could not be read ({why}); if mirqab was upgraded, restart the daemon: systemctl --user restart mirqab.service"
    )
}

pub fn apps_bad_sort(v: &str) -> String {
    format!("mirqab: --sort is cpu, mem or name, not {v}")
}

pub fn bad_pid(v: &str) -> String {
    format!("mirqab: not a process id: {v}")
}

/// `mirqab what`: one labelled line.
pub fn what_line(label: &str, value: &str) -> String {
    format!("{label:<11} {}", plain(value))
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
pub const WHAT_OPEN: &str = "open";

/// A process by name and pid: `browser (2001)`.
pub fn proc_ref(name: &str, pid: u32) -> String {
    format!("{} ({pid})", plain(name))
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

// --- The Apps page (plan 006, part 3b) -------------------------------------

pub const APPS_SEARCH: &str = "Search apps, processes or a pid";
pub const APPS_SORT: &str = "Sort by";
pub const APPS_SORT_CPU: &str = "CPU";
pub const APPS_SORT_MEM: &str = "Memory";
pub const APPS_SORT_NAME: &str = "Name";
pub const APPS_SHOW_PROCESSES: &str = "Show processes";
pub const APPS_EXPAND: &str = "Show its processes";
pub const APPS_COLLAPSE: &str = "Hide its processes";
pub const APPS_LIST: &str = "Apps";
pub const APPS_INSPECTOR: &str = "Inspector";
pub const APPS_REFRESH: &str = "Read again";
pub const APPS_CLOSE_INSPECTOR: &str = "Close the inspector";
pub const APPS_QUIT: &str = "Quit";
pub const APPS_FORCE_QUIT: &str = "Force quit";
pub const APPS_EMPTY: &str = "Nothing matches";
pub const APPS_TOP: &str = "Top apps";
pub const APPS_CPU: &str = "CPU";
pub const APPS_MEMORY: &str = "Memory";
pub const APPS_THREADS: &str = "Threads";
pub const APPS_PROCESSES: &str = "Processes";
pub const APPS_GRAPH_PROCESS: &str = "CPU of this process";
pub const APPS_GRAPH_GROUP: &str = "CPU of this app";
/// A row that left the list while selected (plan 006, D5): the process
/// exited, or its group id now names other processes.
pub const APPS_ROW_GONE: &str = "That row is gone: its processes exited or were replaced";
pub const APPS_NOT_YOURS: &str = "Another user's processes: mirqab does not quit them";
pub const APPS_QUIT_SELF: &str = "Quit closes this window. The daemon keeps running.";
pub const APPS_STILL_RUNNING: &str = "Still running. Force quit ends it at once.";
/// "Idle but heavy" (plan 006, part 4, decision 6): the dim caption after a
/// tagged row's process count. A word, no colour and no icon.
pub const APPS_IDLE_HEAVY: &str = "busy while not in use";
/// The inspector's line under the stats for a tagged group.
pub const APPS_IDLE_HEAVY_LINE: &str =
    "At least 20 % of a core for five minutes with none of its windows in focus";
/// `QUIT_NOT_RECORDED` without the CLI's `mirqab:` prefix.
pub const APPS_NOT_RECORDED: &str =
    "Done, but not recorded: the daemon could not write this action to its history";
pub const APPS_DONE: &str = "Done.";
pub const APPS_ALREADY_GONE: &str = "It had already exited.";
pub const APPS_LEFT_ALONE: &str = "Left alone:";
pub const APPS_ALSO_STOPS_UNKNOWN: &str = "Other services that depend on it may stop too";
/// `mirqab-ui --do` outside a test run.
pub const UI_DO_TEST_ONLY: &str = "mirqab-ui: --do is accepted only with MIRQAB_TEST=1";

/// A selftest step that failed (`mirqab-ui --selftest TAB --do STEPS`).
pub fn selftest_step_failed(step: &str, why: &str) -> String {
    format!("mirqab-ui: --do step '{step}': {why}")
}

pub const SELFTEST_NO_WIDGET: &str = "no visible widget of that name";
pub const SELFTEST_NOT_EDITABLE: &str = "the widget takes no text";
pub const SELFTEST_NOT_ACTIVATABLE: &str = "neither the widget nor a parent can be activated";
pub const SELFTEST_NO_DIALOG: &str = "no dialog is open";
pub const SELFTEST_BAD_STEP: &str =
    "not a step: type NAME TEXT, activate NAME, respond ID, wait NAME or pause MS";

/// A group's kind as a word, under its name in the inspector.
pub fn apps_kind(kind: &str) -> &'static str {
    match kind {
        "app" => "App",
        "service" => "Service",
        "flatpak" => "Flatpak app",
        "container" => "Container",
        "terminal" => "Terminal job",
        "system" => "Other users",
        _ => "Process",
    }
}

pub fn apps_procs(n: u32) -> String {
    if n == 1 {
        "1 process".to_string()
    } else {
        format!("{n} processes")
    }
}

pub fn apps_pid(pid: u32) -> String {
    format!("pid {pid}")
}

/// A process row's state, shown when it is not running or sleeping.
pub fn apps_state(state: &str) -> String {
    state_word(state).to_string()
}

/// A process row in the flat list: its group's name after it.
pub fn apps_in_group(group: &str) -> String {
    format!("in {}", plain(group))
}

/// The accessible label of a group row; a tagged group says so last.
pub fn apps_group_label(
    name: &str,
    cpu: &str,
    memory: &str,
    procs: u32,
    idle_heavy: bool,
) -> String {
    let mut out = format!(
        "{}, CPU {cpu}, memory {memory}, {}",
        plain(name),
        apps_procs(procs)
    );
    if idle_heavy {
        out.push_str(", ");
        out.push_str(APPS_IDLE_HEAVY);
    }
    out
}

/// The accessible label of a process row.
pub fn apps_process_label(name: &str, pid: u32, cpu: &str, memory: &str) -> String {
    format!("{}, pid {pid}, CPU {cpu}, memory {memory}", plain(name))
}

/// The confirmation's line for one process a Quit signals.
pub fn apps_confirm_signal(name: &str, pid: u32, via: Option<u32>) -> String {
    match via {
        Some(t) => format!("{} ({pid}), asked for its thread {t}", plain(name)),
        None => format!("{} ({pid})", plain(name)),
    }
}

/// The confirmation's line for a service stopped with systemctl.
pub fn apps_confirm_stop(unit: &str, procs: usize) -> String {
    format!(
        "Stops {} with systemctl ({})",
        plain(unit),
        apps_procs(procs as u32)
    )
}

/// The confirmation's line for Force quit's second command on a service.
pub fn apps_confirm_unit_kill(unit: &str) -> String {
    format!(
        "Then ends whatever is left of {} at once (SIGKILL)",
        plain(unit)
    )
}

/// The pids under a stop's line.
pub fn apps_confirm_pids(pids: &[u32]) -> String {
    let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
    format!("  pids {}", pids.join(" "))
}

/// A unit a stop takes with it. `deeper` for a unit that is no service,
/// scope or slice (a target), whose own dependents stop too, unlisted
/// (review of PR #18, S3).
pub fn apps_confirm_also_stops(unit: &str, procs: usize, deeper: bool) -> String {
    let tail = if deeper {
        " and what depends on it"
    } else {
        ""
    };
    format!(
        "  Also stops {} ({}){tail}",
        plain(unit),
        apps_procs(procs as u32)
    )
}

/// One process a Quit leaves alone, and why.
pub fn apps_left_alone_line(pid: u32, name: &str, reason: &str) -> String {
    format!("  {} ({pid}): {}", plain(name), plain(reason))
}

/// What an act on a row came to, once its sheet has closed (review of
/// PR #19): `Example Browser: Done.`
pub fn apps_outcome(name: &str, text: &str) -> String {
    format!("{}: {text}", plain(name))
}

/// `--selftest` reaching an interface that is already running (review
/// of PR #19): a selftest never joins one.
pub const UI_SELFTEST_NOT_FORWARDED: &str =
    "mirqab-ui: --selftest is not passed to a running interface";

/// `--selftest` with a tab id that names no tab (audit 2026-11-21, L57).
pub const UI_SELFTEST_UNKNOWN_TAB: &str = "mirqab-ui: --selftest takes a tab id";

/// A warning the daemon gave with the plan (the sound server, a portal).
pub fn apps_warning(text: &str) -> String {
    format!("Warning: {}", plain(text))
}

// --- Ports (plan 007) --------------------------------------------------------

/// A `ports` subscription or `get` whose params are not the stream's.
pub const PORTS_PARAMS: &str =
    "params are \"udp\" and \"lite\", each true or false, and nothing else";

/// `status`'s reason when the socket tables cannot be read.
pub fn ports_unavailable(why: &str) -> String {
    format!("proc/net/tcp cannot be read: {why}")
}

/// What a scope means, in a sentence: `mirqab port`, the page's Reach
/// line. mirqab reads bind addresses, never firewall rules.
pub fn reach_sentence(scope: &str) -> &'static str {
    match scope {
        "local" => "Only this machine can connect.",
        "all" => {
            "Listening on every interface, so other devices on your network can connect, unless a firewall blocks it. Bind it to 127.0.0.1 to keep it on this machine."
        }
        "lan" => {
            "Listening on this machine's network address, so other devices on your network can connect, unless a firewall blocks it."
        }
        "bridge" => {
            "Listening on a container bridge address: containers on that bridge can connect, and so can anything the bridge forwards."
        }
        "vpn" => "Listening on a VPN address: devices on that VPN can connect.",
        "link" => {
            "Listening on a link-local address: devices on the same network segment can connect."
        }
        "other" => {
            "Listening on the address of another network, such as a virtual link: whatever is on that network can connect."
        }
        _ => {
            "Listening on an address mirqab cannot place on an interface; treat it as reachable from outside."
        }
    }
}

/// `mirqab ports`: the header and one row per listener, values already
/// formatted.
pub const PORTS_HEADER: &str = "PORT    PROTO  BIND             REACH    PROCESS               PROJECT          LAUNCHER      UP";

#[allow(clippy::too_many_arguments)]
pub fn ports_row(
    port: u16,
    proto: &str,
    bind: &str,
    reach: &str,
    process: &str,
    project: &str,
    launcher: &str,
    up: &str,
) -> String {
    let port = format!(":{port}");
    let (bind, process) = (fit(bind, 16), fit(process, 21));
    let (project, launcher) = (fit(project, 16), fit(launcher, 13));
    format!(
        "{port:<7} {proto:<6} {bind:<16} {reach:<8} {process:<21} {project:<16} {launcher:<13} {up}"
    )
}

/// A column `mirqab ports` has no value for.
pub const PORTS_NONE: &str = "-";
/// The PROCESS cell of a port of root's.
pub const PORTS_ROOT: &str = "root";
/// The PROCESS cell of a port of yours whose process was not found.
pub const PORTS_YOURS_UNKNOWN: &str = "yours, not found";
/// The same, in a sentence (`mirqab port`, the page).
pub const PORTS_YOURS_UNKNOWN_LONG: &str = "a process of yours mirqab could not find";

/// The PROCESS cell of a process of yours: its name and pid.
pub fn ports_process(name: &str, pid: u32) -> String {
    format!("{} ({pid})", plain(name))
}

/// The PROCESS cell of another user's port.
pub fn ports_other(uid: Option<u32>) -> String {
    match uid {
        Some(uid) => format!("uid {uid}"),
        None => "another user".to_string(),
    }
}

/// `mirqab port`: what root's port shows for its process.
pub const PORTS_ROOT_LONG: &str = "owned by root; mirqab cannot see which process";

/// `mirqab port`: another user's port.
pub fn ports_other_long(uid: Option<u32>) -> String {
    match uid {
        Some(uid) => format!("owned by uid {uid}; mirqab cannot see which process"),
        None => "owned by another user; mirqab cannot see which process".to_string(),
    }
}

/// `mirqab ports`' last lines: one per port of yours the network reaches.
pub fn ports_exposed_line(port: u16, proto: &str, bind: &str, process: &str) -> String {
    format!(
        "exposed: :{port}/{proto} {} listens on {}, so the network can reach it unless a firewall blocks it",
        plain(process),
        plain(bind)
    )
}

/// `mirqab port`'s labels.
pub const PORT_PORT: &str = "Port";
pub const PORT_REACH: &str = "Reach";
pub const PORT_PROCESS: &str = "Process";
pub const PORT_LABEL: &str = "Label";
pub const PORT_PROJECT: &str = "Project";
pub const PORT_FRAMEWORK: &str = "Framework";
pub const PORT_SHELL: &str = "Shell";
pub const PORT_LAUNCHER: &str = "Launcher";
pub const PORT_UNIT: &str = "Unit";
pub const PORT_CONTAINER: &str = "Container";
pub const PORT_UP: &str = "Up";
pub const PORT_URL: &str = "URL";

/// `mirqab port`: one labelled line.
pub fn port_line(label: &str, value: &str) -> String {
    format!("{label:<10} {}", plain(value))
}

/// `mirqab port`'s Port line: the port, the protocol, the bound address
/// and its families.
pub fn port_where(port: u16, proto: &str, bind: &str, families: &[u8]) -> String {
    let fam = match families {
        [4] => "IPv4",
        [6] => "IPv6",
        _ => "IPv4 and IPv6",
    };
    format!("{port}/{proto} on {} ({fam})", plain(bind))
}

/// `mirqab port`'s Project line: the name and where it is.
pub fn port_project(name: &str, path: &str) -> String {
    format!("{} ({})", plain(name), plain(path))
}

/// Nothing listens on the port asked about.
pub fn port_not_listening(port: u16, proto: &str) -> String {
    format!("nothing listens on port {port}/{proto}")
}

pub fn bad_port(v: &str) -> String {
    format!("mirqab: not a port (1 to 65535): {v}")
}

pub fn bad_proto(v: &str) -> String {
    format!("mirqab: --proto is tcp or udp, not {v}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_badge_says_what_it_counts() {
        assert_eq!(tile_badge("ports", 1), "1 port listening");
        assert_eq!(tile_badge("ports", 3), "3 ports listening");
        assert_eq!(tile_badge("agents", 1), "1 session needs you");
        assert_eq!(tile_badge("agents", 2), "2 sessions need you");
        assert_eq!(tile_badge("trends", 4), "4");
    }

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
    fn plain_text_has_no_control_characters() {
        let p = plain("a\x1b[2Jb\nc\u{9b}d\u{202e}e\u{7f}");
        assert!(!p.chars().any(char::is_control), "{p:?}");
        assert!(!p.contains('\u{202e}'));
        assert_eq!(p, "a\u{fffd}[2Jb\u{fffd}c\u{fffd}d\u{fffd}e\u{fffd}");
        assert!(matches!(
            plain("Web Content"),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(apps_row("x\ny", "1%", "1 MB", 1).lines().count(), 1);
        assert_eq!(what_line("command", "a\rb"), "command     a\u{fffd}b");
        assert_eq!(proc_ref("a\x1bb", 1), "a\u{fffd}b (1)");
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
        assert_eq!(status_focus(Some("hyprland")), "focus       hyprland");
    }

    #[test]
    fn the_idle_column_lines_up_with_its_header() {
        let row = apps_row_idle(&apps_row("cargo", "91%", "2 GB", 2), true);
        assert_eq!(row.find("idle"), APPS_HEADER_IDLE.find("IDLE"));
        assert_eq!(apps_row_idle("x", false), "x");
    }
}
