//! Every string a person reads, in the interface, the CLI and the daemon's
//! data (architecture §7.2). English only for now; a translation replaces
//! this module's body and touches no call site.

pub const APP_NAME: &str = "mirqab";

/// One command of the CLI: its name, its usage lines (after `mirqab `),
/// a one-line summary for the usage list, the paragraph `--help` adds, and
/// the flags completion offers. The usage text, each command's help,
/// `docs/cli.md` and the shell completions are all made from this table
/// (plan 005).
pub struct CliCommand {
    pub name: &'static str,
    pub usage: &'static [&'static str],
    pub summary: &'static str,
    pub help: &'static str,
    pub flags: &'static [&'static str],
}

pub const CLI_COMMANDS: &[CliCommand] = &[
    CliCommand {
        name: "daemon",
        usage: &["daemon [--root DIR] [--socket PATH]"],
        summary: "run the daemon (systemd starts it from mirqab.socket)",
        help: "Serves the socket. --root reads system state from DIR instead of / (tests use fixture trees). Run by hand, it prints `ready <socket>` once it listens.",
        flags: &["--root"],
    },
    CliCommand {
        name: "status",
        usage: &["status [--json]"],
        summary: "version, socket, subscribers and the daemon's cost",
        help: "Prints the version, the socket, each stream's subscribers and interval, and what the daemon costs.",
        flags: &["--json"],
    },
    CliCommand {
        name: "open",
        usage: &["open [TAB] [--window]"],
        summary: "show the window, on TAB",
        help: "Shows the window, on TAB if given. --window opens a normal window instead of the popover.",
        flags: &["--window", "--dry-run"],
    },
    CliCommand {
        name: "toggle",
        usage: &["toggle [TAB]"],
        summary: "show the window, or hide it if it is shown",
        help: "Shows the window, or hides it if it is shown.",
        flags: &["--dry-run"],
    },
    CliCommand {
        name: "close",
        usage: &["close"],
        summary: "hide the window",
        help: "Hides the window.",
        flags: &["--dry-run"],
    },
    CliCommand {
        name: "history",
        usage: &["history SERIES... [--range R] [--json]", "history --list"],
        summary: "stored minutes (R: 1h, 24h, 7d, 30d; default 1h)",
        help: "Prints the average and peak of each bucket of stored minutes. R is 1h (default), 24h, 7d or 30d. --list names the series history keeps.",
        flags: &["--range", "--json", "--list"],
    },
    CliCommand {
        name: "load",
        usage: &["load [--json]"],
        summary: "CPU, load, memory, pressure and the thermal word",
        help: "CPU (over the last seconds), load averages, memory, swap, pressure and the thermal word.",
        flags: &["--json"],
    },
    CliCommand {
        name: "sensors",
        usage: &["sensors [--json]"],
        summary: "temperatures, GPUs and power",
        help: "Every temperature sensor, each GPU, and package and battery power.",
        flags: &["--json"],
    },
    CliCommand {
        name: "bar",
        usage: &[
            "bar ITEM [--watch] [--interval SEC] [--format waybar|plain|json]",
            "bar --list",
        ],
        summary: "a reading for a status bar (mirqab bar --list names the items)",
        help: "Prints one reading of ITEM for a status bar and exits. With --watch it prints a line each time the reading changes, at most once per interval (default 5 s, from 2 s to an hour), until its output is closed. --format waybar (default) prints Waybar's JSON; plain the text only; json every field.",
        flags: &["--watch", "--interval", "--format", "--list"],
    },
    CliCommand {
        name: "completions",
        usage: &["completions bash|zsh|fish"],
        summary: "a shell completion script",
        help: "Prints a completion script for the shell. For bash: mirqab completions bash > ~/.local/share/bash-completion/completions/mirqab",
        flags: &[],
    },
    CliCommand {
        name: "help",
        usage: &["help [COMMAND]", "help --markdown"],
        summary: "this text, or one command's",
        help: "Prints this text, or one command's. --markdown prints every command as Markdown (docs/cli.md is made from it).",
        flags: &["--markdown"],
    },
];

/// The options every command takes.
pub const CLI_GLOBAL: &[(&str, &str)] = &[
    (
        "--socket PATH",
        "the daemon's socket (default $XDG_RUNTIME_DIR/mirqab/daemon.sock, or $MIRQAB_SOCKET)",
    ),
    ("--help", "help for the command"),
    ("--version", "print the version"),
];

/// Exit codes, for the usage text and `docs/cli.md`.
pub const CLI_EXITS: &str = "Exit codes: 0 done, 1 refused or failed, 2 no daemon, 64 usage error.";

pub fn cli_command(name: &str) -> Option<&'static CliCommand> {
    CLI_COMMANDS.iter().find(|c| c.name == name)
}

/// `mirqab --help`.
pub fn cli_usage() -> String {
    let mut out = String::from("usage: mirqab <command> [options]\n\ncommands:\n");
    for c in CLI_COMMANDS {
        for (i, u) in c.usage.iter().enumerate() {
            let summary = if i == 0 { c.summary } else { "" };
            if u.len() < 37 {
                out.push_str(format!("  {u:<37} {summary}").trim_end());
            } else {
                out.push_str(format!("  {u}  {summary}").trim_end());
            }
            out.push('\n');
        }
    }
    out.push_str("\noptions for every command:\n");
    for (o, what) in CLI_GLOBAL {
        out.push_str(&format!("  {o:<15} {what}\n"));
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
    for (i, u) in c.usage.iter().enumerate() {
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
        for u in c.usage {
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

/// `mirqab completions SHELL`: commands, and each command's flags.
pub fn cli_completions(shell: &str) -> Option<String> {
    let names: Vec<&str> = CLI_COMMANDS.iter().map(|c| c.name).collect();
    let globals = "--socket --help --version";
    Some(match shell {
        "bash" => {
            let mut cases = String::new();
            for c in CLI_COMMANDS {
                cases.push_str(&format!(
                    "        {}) opts=\"{} {globals}\" ;;\n",
                    c.name,
                    c.flags.join(" ")
                ));
            }
            format!(
                "# mirqab bash completion (mirqab completions bash)\n\
                 _mirqab() {{\n    local cur=${{COMP_WORDS[COMP_CWORD]}} opts\n    \
                 if [[ $COMP_CWORD -eq 1 ]]; then\n        \
                 COMPREPLY=($(compgen -W \"{} {globals}\" -- \"$cur\"))\n        return\n    fi\n    \
                 case ${{COMP_WORDS[1]}} in\n{cases}        *) opts=\"{globals}\" ;;\n    esac\n    \
                 COMPREPLY=($(compgen -W \"$opts\" -- \"$cur\"))\n}}\ncomplete -F _mirqab mirqab\n",
                names.join(" ")
            )
        }
        "zsh" => {
            let mut cases = String::new();
            for c in CLI_COMMANDS {
                cases.push_str(&format!(
                    "    ({}) compadd -- {} {globals} ;;\n",
                    c.name,
                    c.flags.join(" ")
                ));
            }
            format!(
                "#compdef mirqab\n# mirqab zsh completion (mirqab completions zsh)\n\
                 if (( CURRENT == 2 )); then\n  compadd -- {} {globals}\n  return\nfi\n\
                 case $words[2] in\n{cases}    (*) compadd -- {globals} ;;\nesac\n",
                names.join(" ")
            )
        }
        "fish" => {
            let mut out = String::from(
                "# mirqab fish completion (mirqab completions fish)\ncomplete -c mirqab -f\n",
            );
            for c in CLI_COMMANDS {
                out.push_str(&format!(
                    "complete -c mirqab -n '__fish_use_subcommand' -a {} -d '{}'\n",
                    c.name,
                    c.summary.replace('\'', "")
                ));
                for f in c.flags {
                    out.push_str(&format!(
                        "complete -c mirqab -n '__fish_seen_subcommand_from {}' -l {}\n",
                        c.name,
                        f.trim_start_matches("--")
                    ));
                }
            }
            for g in ["socket", "help", "version"] {
                out.push_str(&format!("complete -c mirqab -l {g}\n"));
            }
            out
        }
        _ => return None,
    })
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
  --window        open as a normal window instead of the popover
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

/// The tray's icon name in the icon theme (`dist/icons/`).
pub const TRAY_ICON: &str = "mirqab-symbolic";
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
                for f in c.flags {
                    assert!(script.contains(f.trim_start_matches("--")), "{shell}: {f}");
                }
            }
        }
        assert!(cli_completions("tcsh").is_none());
    }

    #[test]
    fn every_command_in_the_usage_has_help() {
        for c in [
            "daemon",
            "status",
            "open",
            "toggle",
            "close",
            "history",
            "load",
            "sensors",
            "bar",
            "completions",
            "help",
        ] {
            assert!(
                cli_usage().contains(&format!("  {c}")),
                "{c} missing from usage"
            );
            assert!(command_help(c).is_some(), "{c} has no help");
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
