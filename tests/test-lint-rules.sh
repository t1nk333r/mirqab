# tests/test-lint-rules.sh — each grep rule of tools/lint.sh fails on a
# violation planted in a scratch tree, and a clean tree passes.
# mirqab:covers tools/lint.sh

sandbox

# tree NAME — a fresh scratch tree with clean files in every place a rule
# looks; prints its path.
tree() {
    local t="$SANDBOX/$1"
    mkdir -p "$t"/crates/{mirqab,mirqab-core,mirqab-view,mirqab-ui}/src \
        "$t/crates/mirqab-ui/src/tabs/system" "$t/crates/mirqab-ui/resources" "$t/dist"
    printf 'pub fn read(root: &Root) { root.read("proc/stat"); }\n' >"$t/crates/mirqab-core/src/sampler.rs"
    printf 'pub fn path() -> &%sstatic str { "/proc/stat" }\n' "'" >"$t/crates/mirqab-core/src/root.rs"
    printf 'pub fn run() { std::process::Command::new("true"); }\n' >"$t/crates/mirqab-core/src/exec.rs"
    printf 'pub fn run() { let _ = "pkexec"; }\n' >"$t/crates/mirqab/src/elevate.rs"
    printf 'pub const ACCENT: &str = "#ff8800";\n' >"$t/crates/mirqab-view/src/theme.rs"
    cat >"$t/crates/mirqab-ui/src/app.rs" <<'EOF'
fn start() {
    label.set_text(&strings::title());
    // mirqab:ui-timeout
    glib::timeout_add_seconds_local_once(60, quit);
}
EOF
    printf 'fn page() { client.subscribe("system", 2000); }\n' >"$t/crates/mirqab-ui/src/tabs/system/page.rs"
    cat >"$t/crates/mirqab-ui/resources/style.css" <<'EOF'
/* mirqab:tokens begin */
:root { --mirqab-series-1: #ff8800; }
/* mirqab:tokens end */
.card { background: var(--card-bg-color); }
EOF
    printf '%s\n' "$t"
}

clean="$(tree clean)"
capture "$REPO/tools/lint.sh" --rules "$clean"
status_is "a clean tree passes" 0 "$STATUS"
is "a clean tree reports nothing" "" "$OUTPUT"

# plant LABEL FILE CONTENT EXPECTED — write CONTENT to FILE in a fresh tree;
# the rules must fail and name EXPECTED.
n=0
plant() {
    n=$((n + 1))
    local t
    t="$(tree "case$n")"
    mkdir -p "$(dirname "$t/$2")"
    printf '%s\n' "$3" >>"$t/$2"
    capture "$REPO/tools/lint.sh" --rules "$t"
    status_is "$1: fails" 1 "$STATUS"
    like "$1: names the rule" "$OUTPUT" "$4"
    like "$1: names the file" "$OUTPUT" "$2"
}

plant "sudo in the daemon" crates/mirqab/src/cli.rs 'fn x() { run("sudo"); }' "sudo or pkexec"
plant "pkexec in a unit" dist/mirqab.service 'ExecStart=pkexec /usr/bin/x' "sudo or pkexec"
plant "a literal /proc path" crates/mirqab-core/src/samplers.rs 'fn x() { read("/proc/stat"); }' "literal system path"
plant "a literal /sys path" crates/mirqab-core/src/fans.rs 'fn x() { read("/sys/class/hwmon"); }' "literal system path"
plant "a kill outside act.rs" crates/mirqab-core/src/ports.rs 'fn x() { libc::kill(1, 15); }' "mutation or command"
plant "remove_file outside act.rs" crates/mirqab-core/src/history.rs 'fn x() { std::fs::remove_file(p); }' "mutation or command"
plant "Command::new outside exec.rs" crates/mirqab/src/cli.rs 'fn x() { Command::new("ls"); }' "mutation or command"
plant "println! in the CLI" crates/mirqab/src/cli.rs 'fn x() { println!("{v}"); }' "print macro outside out.rs"
plant "eprintln! in the daemon" crates/mirqab/src/daemon.rs 'fn x() { eprintln!("no"); }' "print macro outside out.rs"
plant "gtk in the view crate" crates/mirqab-view/src/graph.rs 'use gtk4::prelude::*;' "toolkit named in a headless crate"
plant "glib in the daemon" crates/mirqab/src/main.rs 'fn x() { glib::idle_add(f); }' "toolkit named in a headless crate"
plant "a toolkit dependency" crates/mirqab-core/Cargo.toml 'gtk4 = "0.11"' "toolkit dependency"
plant "hex in a widget" crates/mirqab-ui/src/widgets/graph.rs 'const C: &str = "#ff0000";' "colour literal outside the token table"
plant "RGBA::new in a widget" crates/mirqab-ui/src/widgets/meter.rs 'let c = gdk::RGBA::new(1.0, 0.0, 0.0, 1.0);' "colour literal outside the token table"
plant "hex in a view model" crates/mirqab-view/src/tabs.rs 'const C: &str = "#abc";' "colour literal outside the token table"
plant "hex outside the token block" crates/mirqab-ui/resources/style.css '.x { color: #123456; }' "colour literal outside the token block"
plant "rgba outside the token block" crates/mirqab-ui/resources/style.css '.x { color: rgba(0, 0, 0, 0.5); }' "colour literal outside the token block"
plant "push_blur" crates/mirqab-ui/src/widgets/card.rs 'fn x(s: &Snapshot) { s.push_blur(4.0); }' "blur, shader or GL"
plant "a CSS blur filter" crates/mirqab-ui/resources/style.css '.x { filter: blur(4px); }' "blur, shader or GL"
plant "backdrop-filter" crates/mirqab-ui/resources/style.css '.x { backdrop-filter: none; }' "blur, shader or GL"
plant "a GLArea" crates/mirqab-ui/src/widgets/map.rs 'let a = gtk::GLArea::new();' "blur, shader or GL"
plant "an unmarked timeout" crates/mirqab-ui/src/window.rs 'fn x() { glib::timeout_add_local(d, f); }' "timer without"
plant "an unmarked tick callback" crates/mirqab-ui/src/widgets/pill.rs 'fn x(w: &W) { w.add_tick_callback(f); }' "timer without"
plant "a literal label" crates/mirqab-ui/src/widgets/footer.rs 'let l = gtk::Label::new(Some("Settings"));' "string literal in a widget call"
plant "a literal title" crates/mirqab-ui/src/window.rs 'w.set_title(Some("mirqab"));' "string literal in a widget call"
plant "subscribe_live from a page" crates/mirqab-ui/src/tabs/system/page.rs 'fn x() { client.subscribe_live("system", f); }' "live sampling asked for outside widgets/graph.rs"
plant "a live param from a card" crates/mirqab-ui/src/widgets/card.rs 'let p = json!({"interval": 1000, "live": true});' "live sampling asked for outside widgets/graph.rs"
plant "a live param on a wrapped call" crates/mirqab-ui/src/app.rs "$(printf '%s\n' 'c.subscribe_with(' '    "system",' '    json!({' '        "live": true' '    }),' ');')" "live sampling asked for outside widgets/graph.rs"
plant "an unmarked frame callback with a sample marker" crates/mirqab-ui/src/widgets/pill.rs "$(printf '%s\n' '// mirqab:sample' 'w.add_tick_callback(f);')" "frame callback not marked ui-timeout"
plant "a sampler under a second" crates/mirqab-ui/src/widgets/clock.rs "$(printf '%s\n' '// mirqab:sample' 'glib::timeout_add_local(Duration::from_millis(500), f);')" "a mirqab:sample timer under 1000 ms"
plant "a trailing marker does not mark the next line" crates/mirqab-ui/src/timers.rs "$(printf '%s\n' 'fn a() { glib::timeout_add_local_once(d, f); } // mirqab:ui-timeout' 'fn b() { glib::timeout_add_local(d2, g); }')" "timer without"
plant "an unmarked timeout_future" crates/mirqab-ui/src/widgets/clock.rs 'async fn tick() { glib::timeout_future(Duration::from_millis(50)).await; }' "timer without"
plant "a renamed toolkit dependency" crates/mirqab-view/Cargo.toml 'ui = { package = "gtk4", version = "0.11" }' "toolkit dependency"

# A marker on the line above, with blank lines between, still counts; a
# marker two statements up does not.
t="$(tree marker)"
printf '%s\n' '// mirqab:sample' '' 'glib::timeout_add_local(d, f);' >"$t/crates/mirqab-ui/src/sample.rs"
capture "$REPO/tools/lint.sh" --rules "$t"
status_is "a marker above a blank line counts" 0 "$STATUS"
printf '%s\n' '// mirqab:sample' 'let x = 1;' 'glib::timeout_add_local(d, f);' >"$t/crates/mirqab-ui/src/sample.rs"
capture "$REPO/tools/lint.sh" --rules "$t"
status_is "a marker two statements up does not count" 1 "$STATUS"

# A hash that is not a colour value passes: an issue number in a comment,
# a format string, a CSS id selector, a pseudo-class before an id.
t="$(tree hashes)"
cat >"$t/crates/mirqab-ui/src/notes.rs" <<'EOF'
// fixes issue #123, see plan #001
fn bits(n: u8) -> String { format!("{n:#06b}") }
fn wide(n: u8) -> String { format!("{n:>#06b} {:x>#06x}", n) }
let a = "x"; // plan #001
EOF
cat >>"$t/crates/mirqab-ui/resources/style.css" <<'EOF'
#cafe { color: var(--accent-color); }
button:hover #bad { opacity: 0.5; }
/* issue: #123 */
button:hover,
#bad,
.x {
  color: var(--accent-color);
}
EOF
capture "$REPO/tools/lint.sh" --rules "$t"
status_is "a hash that is not a colour value passes" 0 "$STATUS"
is "and reports nothing" "" "$OUTPUT"
plant "a hex value after a selector with an id" crates/mirqab-ui/resources/style.css '#cafe { color: #abcdef; }' "colour literal outside the token block"
plant "a hex inside a longer string" crates/mirqab-ui/src/widgets/x.rs 'let css = "color: #abcdef;";' "colour literal outside the token table"
plant "compact CSS in a string" crates/mirqab-ui/src/widgets/x.rs 'let css = "color:#abcdef;";' "colour literal outside the token table"
plant "a compact value at the end of a string" crates/mirqab-ui/src/widgets/x.rs 'let css = "background:#fff";' "colour literal outside the token table"
plant "a CSS value on the next line" crates/mirqab-ui/resources/style.css "$(printf '%s\n' '.x {' '  background:' '    #abcdef;' '}')" "colour literal outside the token block"
plant "a multi-line gradient" crates/mirqab-ui/resources/style.css "$(printf '%s\n' '.x {' '  background: linear-gradient(' '    #abc, #def);' '}')" "colour literal outside the token block"
plant "a value before the last brace" crates/mirqab-ui/resources/style.css '.a { color: #abcdef; } .b {' "colour literal outside the token block"

# The graph and the client may name live sampling; a use line, a marker at
# the end of the line, and a one-second sampler pass.
t="$(tree allowed)"
mkdir -p "$t/crates/mirqab-ui/src/widgets"
printf '%s\n' 'fn x() { source.subscribe_live("system", f); }' >"$t/crates/mirqab-ui/src/widgets/graph.rs"
printf '%s\n' 'fn subscribe_live() { params["live"] = json!(true); }' >"$t/crates/mirqab-ui/src/client.rs"
cat >"$t/crates/mirqab-ui/src/timers.rs" <<'EOF'
use gtk::glib::timeout_add_local_once;
fn a() { glib::timeout_add_local_once(d, f); } // mirqab:ui-timeout
// mirqab:sample
fn b() { glib::timeout_add_local(Duration::from_millis(1_000), f); }
// mirqab:ui-timeout
fn c() { w.add_tick_callback(f); }
EOF
capture "$REPO/tools/lint.sh" --rules "$t"
status_is "the graph, the client, use lines, trailing markers and 1 s samplers pass" 0 "$STATUS"
is "and report nothing" "" "$OUTPUT"

# An empty string may be set; it carries no words to translate.
t="$(tree empty)"
printf '%s\n' 'label.set_text("");' >"$t/crates/mirqab-ui/src/clear.rs"
capture "$REPO/tools/lint.sh" --rules "$t"
status_is "an empty string in a widget call is allowed" 0 "$STATUS"

# The exemptions are exact paths: the same file name elsewhere is not exempt.
plant "Command::new in another crate's exec.rs" crates/mirqab-ui/src/exec.rs 'fn x() { Command::new("ls"); }' "mutation or command"
plant "kill in another crate's act.rs" crates/mirqab-view/src/act.rs 'fn x() { libc::kill(1, 9); }' "mutation or command"
plant "a /proc path in another crate's root.rs" crates/mirqab-ui/src/tabs/root.rs 'fn x() { read("/proc/stat"); }' "literal system path"
plant "sudo beside a mention of elevate.rs" crates/mirqab-core/src/note.rs '// see crates/mirqab/src/elevate.rs: it runs sudo' "sudo or pkexec"

# A cargo tree that fails is a lint failure, not an empty answer.
capture "$REPO/tools/lint.sh" --tree mirqab-no-such-crate
status_is "cargo tree failing fails the link rule" 1 "$STATUS"
like "and says so" "$OUTPUT" "cargo tree -p mirqab-no-such-crate failed"
capture env CARGO_DENY=/nonexistent "$REPO/tools/lint.sh" --tree mirqab-proto
status_is "--tree runs only the link rule, and a headless crate passes it" 0 "$STATUS"
