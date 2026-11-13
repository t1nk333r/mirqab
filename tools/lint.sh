#!/usr/bin/env bash
# tools/lint.sh — the lint gate (architecture §16, CLAUDE.md "Gates").
#
#   tools/lint.sh              every check below, on this checkout
#   tools/lint.sh --rules DIR  only the grep rules, on the tree at DIR
#                              (tests/test-lint-rules.sh plants violations)
#   tools/lint.sh --tree CRATE...
#                              only the headless-crate link rule, for CRATEs
#
# Checks: cargo fmt; cargo clippy -D warnings; cargo deny (licences, bans,
# sources; advisories too when CI is set, because it downloads a database);
# the headless crates link no GTK (cargo tree); bash -n and shellcheck on
# every shell file; jq on every JSON file; the grep rules of architecture
# §5.3, §5.4, §7.2, §7.3 and §7.4; and coverage (tools/affected-tests.sh
# --check). CARGO_DENY and SHELLCHECK override the tools' paths.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

fail=0
err() { printf 'lint: %s\n' "$*" >&2; fail=1; }

# The crates that must never name or link the toolkit (architecture §3).
HEADLESS=(mirqab-proto mirqab-core mirqab mirqab-fand mirqab-view)
# The toolkit's crates, by package name (a renamed dependency keeps its
# package name in Cargo.toml and in cargo tree).
TOOLKIT_RE='gtk4|gdk4|gsk4|glib|gio|libadwaita|cairo-rs|gtk4-layer-shell|glib-sys|gobject-sys|gio-sys|gtk4-sys|gdk4-sys|gsk4-sys|pango|pango-sys|gdk-pixbuf|gdk-pixbuf-sys|graphene-rs|graphene-sys|cairo-sys-rs|libadwaita-sys|gtk4-layer-shell-sys'
# The one file allowed to say sudo or pkexec, and the fixed helper scripts
# under dist/ that architecture §8 lists (none yet). Exact paths: the same
# file name in another crate is not exempt.
PRIV_ALLOWED=(crates/mirqab/src/elevate.rs)

# hits RULE — report grep's file:line output on stdin as violations of RULE.
# It runs at the end of a pipeline, in a subshell, so it records each
# violation in a file rather than in a variable.
viol="$(mktemp)"
trap 'rm -f -- "$viol"' EXIT
hits() {
    local rule=$1 line
    while IFS= read -r line; do
        [[ -n $line ]] || continue
        printf 'lint: %s: %s\n' "$rule" "${line#./}" | tee -a "$viol" >&2
    done
}

# rs_files DIR... — the Rust sources under each existing DIR.
rs_files() {
    local d
    for d in "$@"; do
        [[ -d $d ]] && find "$d" -name '*.rs' -type f
    done | sort
}

grep_rules() {
    local root=$1 f
    (
        cd "$root"
        # §5.4: privilege strings in one file only.
        local priv=()
        for f in crates dist; do [[ -d $f ]] && priv+=("$f"); done
        if ((${#priv[@]})); then
            grep -rnwE 'sudo|pkexec' "${priv[@]}" 2>/dev/null |
                grep -vE "^($(printf '%s|' "${PRIV_ALLOWED[@]}" | sed 's/[.]/\\./g; s/|$//')):" |
                hits "sudo or pkexec outside elevate.rs (§5.4)"
        fi

        # §5.3: no literal system path outside root.rs; everything goes
        # through Root so tests can pass a fixture tree.
        rs_files crates/*/src | grep -vx 'crates/mirqab-core/src/root\.rs' |
            xargs -r grep -HnE '"/(proc|sys|etc|run|dev)(/|")' |
            hits "literal system path outside root.rs (§5.3)"

        # §5.4: mutation and process spawning only in act.rs and exec.rs.
        rs_files crates/*/src | grep -vxE 'crates/mirqab-core/src/(act|exec)\.rs' |
            xargs -r grep -HnE '\bkill\(|\bremove_file\b|\bremove_dir(_all)?\b|Command::new' |
            hits "mutation or command outside act.rs/exec.rs (§5.4)"

        # Plan 005: the CLI prints through out.rs, which survives a reader
        # that went away; println! and eprintln! panic there (review of
        # PR #12).
        rs_files crates/mirqab/src | grep -vx 'crates/mirqab/src/out\.rs' |
            xargs -r grep -HnE '\be?print(ln)?!' |
            hits "print macro outside out.rs; use crate::out (plan 005)"

        # §3, §7.2: the headless crates never name the toolkit.
        local c dirs=()
        for c in "${HEADLESS[@]}"; do dirs+=("crates/$c"); done
        rs_files "${dirs[@]}" |
            xargs -r grep -HnwE 'gtk4?|gdk4?|gsk4?|adw|libadwaita|glib|gio|cairo' |
            hits "toolkit named in a headless crate (§3)"
        for c in "${dirs[@]}"; do
            [[ -f $c/Cargo.toml ]] || continue
            grep -nE "^($TOOLKIT_RE)\b|package[[:space:]]*=[[:space:]]*\"($TOOLKIT_RE)\"" "$c/Cargo.toml" |
                sed "s#^#$c/Cargo.toml:#" | hits "toolkit dependency in a headless crate (§3)"
        done

        # §7.3: colour literals only in the token table and the token block.
        # In Rust a colour is a value inside an open string literal (an even
        # number of quotes before it, so `"x"; // plan #001` passes): a hash
        # not after a format spec's `:<^>+-`, or `:#hex` that does not run
        # straight into `}` (`"color:#abc;"` fails, `{n:#06b}` passes); or
        # rgb()/rgba() or RGBA::new anywhere (review of PR #4).
        rs_files crates/mirqab-ui/src crates/mirqab-view/src | grep -v '^crates/mirqab-view/src/theme\.rs$' |
            xargs -r grep -HnE '^([^"]*"[^"]*")*[^"]*"(([^"]*[^":<^>+-])?#[0-9a-fA-F]{3,8}\b|[^"]*:#[0-9a-fA-F]{3,8}([^0-9a-zA-Z_}]|$))|\brgba?\(|RGBA::new' |
            hits "colour literal outside the token table (§7.3)"
        local css=crates/mirqab-ui/resources/style.css
        if [[ -f $css ]]; then
            # In CSS a colour is a value: what stands inside a rule's braces,
            # counted across lines, with comments cut away. Selectors (`#cafe
            # {`, `button:hover #id`) are outside every brace (review of
            # PR #4: a value on the next line or before the last `{` passed).
            awk -v f="$css" '
                /mirqab:tokens begin/ { tok = 1; next }
                /mirqab:tokens end/ { tok = 0; next }
                tok { next }
                {
                    v = ""; l = $0; gsub(/\/\*[^*]*\*\//, "", l)
                    n = split(l, ch, "")
                    for (i = 1; i <= n; i++) {
                        if (ch[i] == "{") depth++
                        else if (ch[i] == "}") depth--
                        else if (depth > 0) v = v ch[i]
                    }
                }
                v ~ /#[0-9a-fA-F]{3,8}([^0-9a-zA-Z_-]|$)/ || v ~ /rgba?\(/ { print f ":" NR ":" $0 }
            ' "$css" | hits "colour literal outside the token block (§7.3)"
        fi

        # §7.4: no blur, no shaders, no GL.
        # The widget tests name BlurNode to check that none is drawn, so the
        # rule reads the sources and the stylesheet, not crates/*/tests.
        local draw=()
        for f in crates/mirqab-ui/src crates/mirqab-ui/resources; do [[ -d $f ]] && draw+=("$f"); done
        if ((${#draw[@]})); then
            grep -rnE 'push_blur|BlurNode|blur\(|backdrop-filter|ShaderNode|GLShader|GLArea' \
                --include='*.rs' --include='*.css' "${draw[@]}" |
                hits "blur, shader or GL (§7.4)"
        fi

        # §7.2: every timer says whether it is a one-shot or a sampler, on
        # the line above or at the end of its own line; `use` lines name a
        # function, they start no timer. A frame callback may only be a
        # one-shot transition, and a repeating sampler with a literal
        # interval may not go under 1000 ms (review of PR #1).
        local timer_awk='
            /^[[:space:]]*(pub[[:space:]]+)?use[[:space:]]/ { prev = $0; next }
            /timeout_add|timeout_future|interval_stream|add_tick_callback/ {
                if ($0 ~ /\/\/ mirqab:ui-timeout/ || prev ~ /\/\/ mirqab:ui-timeout/) m = "ui"
                else if ($0 ~ /\/\/ mirqab:sample/ || prev ~ /\/\/ mirqab:sample/) m = "sample"
                else m = ""
                if (want == "marker" && (m == "" || (m == "sample" && $0 ~ /add_tick_callback/)))
                    print f ":" NR ":" $0
                if (want == "floor" && m == "sample") {
                    v = $0
                    if (sub(/.*from_millis\(/, "", v)) {
                        sub(/\).*/, "", v); gsub(/_/, "", v)
                        if (v ~ /^[0-9]+$/ && v + 0 < 1000) print f ":" NR ":" $0
                    }
                }
            }
            # Only a line that is all comment marks the next line: a trailing
            # marker belongs to its own line (review of PR #5).
            /[^[:space:]]/ { prev = ($0 ~ /^[[:space:]]*\/\//) ? $0 : "" }'
        rs_files crates/mirqab-ui/src | while IFS= read -r f; do
            awk -v f="$f" -v want=marker "$timer_awk" "$f"
        done | hits "timer without a mirqab:ui-timeout or mirqab:sample marker, or a frame callback not marked ui-timeout (§7.2)"
        rs_files crates/mirqab-ui/src | while IFS= read -r f; do
            awk -v f="$f" -v want=floor "$timer_awk" "$f"
        done | hits "a mirqab:sample timer under 1000 ms (§7.2)"

        # §7.2: user-visible strings come from mirqab_proto::strings.
        rs_files crates/mirqab-ui/src |
            xargs -r grep -HnE '(Label::new|set_label|set_title|set_subtitle|set_text|set_tooltip_text|set_description)\(\s*(Some\(\s*)?"[^"]' |
            hits "string literal in a widget call; use mirqab_proto::strings (§7.2)"

        # §5.3: only the Graph widget asks for live sampling. The client
        # defines `subscribe_live` and the `"live"` parameter; everything else
        # in the interface may name neither, however a call is wrapped.
        rs_files crates/mirqab-ui/src |
            grep -vxE 'crates/mirqab-ui/src/(client|widgets/graph)\.rs' |
            xargs -r grep -HnE 'subscribe_live|"live"' |
            hits "live sampling asked for outside widgets/graph.rs (§5.3)"
    )
    [[ ! -s $viol ]]
}

if [[ ${1:-} == --rules ]]; then
    [[ -d ${2:-} ]] || { echo "usage: tools/lint.sh --rules DIR" >&2; exit 2; }
    grep_rules "$2" || fail=1
    exit "$fail"
fi

# §3: the headless crates link no toolkit, whatever their sources say. A
# cargo tree that fails is a failure, not an empty answer.
tree_rule() {
    local c out
    for c in "$@"; do
        # Build and dev dependencies count too: a test or build script that
        # links GTK would put it in the crate's build.
        if ! out="$(cargo tree --locked -p "$c" -e normal,build,dev --prefix none 2>&1)"; then
            err "cargo tree -p $c failed: $(tail -n1 <<<"$out") (§3)"
            continue
        fi
        linked="$(awk '{ print $1 }' <<<"$out" |
            grep -xE "$TOOLKIT_RE" | sort -u || true)"
        [[ -z $linked ]] || err "$c links the toolkit: $(paste -sd' ' <<<"$linked") (§3)"
    done
}

if [[ ${1:-} == --tree ]]; then
    shift
    tree_rule "$@"
    exit "$fail"
fi

# Rust: format, clippy, dependencies.
cargo fmt --all --check || err "cargo fmt --check failed (run cargo fmt --all)"
cargo clippy --workspace --all-targets --locked --quiet -- -D warnings || err "cargo clippy failed"
deny_checks=(licenses bans sources)
[[ -z ${CI:-} ]] || deny_checks+=(advisories)
if command -v "${CARGO_DENY:-cargo-deny}" >/dev/null 2>&1; then
    "${CARGO_DENY:-cargo-deny}" --log-level error check "${deny_checks[@]}" || err "cargo deny failed"
else
    err "cargo-deny not found (set CARGO_DENY or install cargo-deny)"
fi

tree_rule "${HEADLESS[@]}"

# Shell: syntax and shellcheck on every tracked or new shell file.
mapfile -t shell_files < <(
    git ls-files -co --exclude-standard | while IFS= read -r f; do
        [[ -f $f ]] || continue
        case $f in
        *.sh) printf '%s\n' "$f" ;;
        *) head -n1 "$f" 2>/dev/null | grep -qE '^#!.*\b(ba)?sh\b' && printf '%s\n' "$f" ;;
        esac
    done
)
SHELLCHECK=${SHELLCHECK:-shellcheck}
for f in "${shell_files[@]}"; do
    bash -n "$f" || err "bash -n: $f"
done
if command -v "$SHELLCHECK" >/dev/null 2>&1; then
    ((${#shell_files[@]} == 0)) || "$SHELLCHECK" -x --severity=warning "${shell_files[@]}" || err "shellcheck failed"
else
    err "shellcheck not found (set SHELLCHECK)"
fi

# JSON: every file parses.
while IFS= read -r f; do
    [[ -f $f ]] || continue
    jq empty "$f" 2>/dev/null || err "invalid JSON: $f"
done < <(git ls-files -co --exclude-standard -- '*.json')

grep_rules . || fail=1

# Coverage: every file is covered by a test, global or exempt.
tools/affected-tests.sh --check || err "coverage check failed"

((fail == 0)) && echo "lint OK"
exit "$fail"
