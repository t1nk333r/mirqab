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

        # §3, §7.2: the headless crates never name the toolkit.
        local c dirs=()
        for c in "${HEADLESS[@]}"; do dirs+=("crates/$c"); done
        rs_files "${dirs[@]}" |
            xargs -r grep -HnwE 'gtk4?|gdk4?|gsk4?|adw|libadwaita|glib|gio|cairo' |
            hits "toolkit named in a headless crate (§3)"
        for c in "${dirs[@]}"; do
            [[ -f $c/Cargo.toml ]] || continue
            grep -nE '^(gtk4|gdk4|gsk4|glib|gio|libadwaita|cairo-rs|gtk4-layer-shell)\b' "$c/Cargo.toml" |
                sed "s#^#$c/Cargo.toml:#" | hits "toolkit dependency in a headless crate (§3)"
        done

        # §7.3: colour literals only in the token table and the token block.
        # In Rust a colour is a value inside a string literal: a hash after a
        # quote (not `{:#06b}`, not `// issue #123`), or rgb()/rgba() or
        # RGBA::new anywhere.
        rs_files crates/mirqab-ui/src crates/mirqab-view/src | grep -v '^crates/mirqab-view/src/theme\.rs$' |
            xargs -r grep -HnE '"([^"]*[^"{:])?#[0-9a-fA-F]{3,8}\b|\brgba?\(|RGBA::new' |
            hits "colour literal outside the token table (§7.3)"
        local css=crates/mirqab-ui/resources/style.css
        if [[ -f $css ]]; then
            # In CSS a colour is a value: after a colon, once everything up
            # to the last `{` (the selector, with its pseudo-classes and
            # `#id`s) is cut away.
            awk -v f="$css" '
                /mirqab:tokens begin/ { tok = 1; next }
                /mirqab:tokens end/ { tok = 0; next }
                tok { next }
                { v = $0; sub(/.*\{/, "", v) }
                v ~ /:[^;]*#[0-9a-fA-F]{3,8}([^0-9a-zA-Z_-]|$)/ || v ~ /rgba?\(/ { print f ":" NR ":" $0 }
            ' "$css" | hits "colour literal outside the token block (§7.3)"
        fi

        # §7.4: no blur, no shaders, no GL.
        if [[ -d crates/mirqab-ui ]]; then
            grep -rnE 'push_blur|BlurNode|blur\(|backdrop-filter|ShaderNode|GLShader|GLArea' \
                --include='*.rs' --include='*.css' crates/mirqab-ui |
                hits "blur, shader or GL (§7.4)"
        fi

        # §7.2: every timer says whether it is a one-shot or a sampler.
        rs_files crates/mirqab-ui/src | while IFS= read -r f; do
            awk -v f="$f" '
                /timeout_add|add_tick_callback/ && prev !~ /\/\/ mirqab:(ui-timeout|sample)/ { print f ":" NR ":" $0 }
                /[^[:space:]]/ { prev = $0 }
            ' "$f"
        done | hits "timer without a mirqab:ui-timeout or mirqab:sample marker (§7.2)"

        # §7.2: user-visible strings come from mirqab_proto::strings.
        rs_files crates/mirqab-ui/src |
            xargs -r grep -HnE '(Label::new|set_label|set_title|set_subtitle|set_text|set_tooltip_text|set_description)\(\s*(Some\(\s*)?"[^"]' |
            hits "string literal in a widget call; use mirqab_proto::strings (§7.2)"

        # §5.3: only the Graph widget asks for live sampling.
        rs_files crates/mirqab-ui/src/tabs |
            xargs -r grep -HnE 'subscribe.*\blive\b|\blive\b.*subscribe' |
            hits "a page passes live to subscribe; only widgets/graph.rs may (§5.3)"
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
        if ! out="$(cargo tree --locked -p "$c" -e normal --prefix none 2>&1)"; then
            err "cargo tree -p $c failed: $(tail -n1 <<<"$out") (§3)"
            continue
        fi
        linked="$(awk '{ print $1 }' <<<"$out" |
            grep -xE 'gtk4|gdk4|gsk4|glib|gio|libadwaita|cairo-rs|gtk4-layer-shell' | sort -u || true)"
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
