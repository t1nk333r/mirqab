# tests/test-rust.sh — cargo test for the crates a change touched and the
# crates that depend on them; every crate when run without --affected or
# when shared infrastructure changed. No display: mirqab-ui's widget tests
# run under Broadway in tests/test-ui-widgets.sh (plan 001 phase D).
# mirqab:covers crates/mirqab-proto/** crates/mirqab-core/** crates/mirqab-view/**
# mirqab:covers crates/mirqab-fand/** crates/mirqab/** crates/mirqab-ui/**
# mirqab:covers-shared

sandbox

# members — the workspace's crates, one per line.
members() {
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$REPO/Cargo.toml" |
        jq -r '.packages[].name'
}

# dependents CRATE... — the given crates and every workspace crate that
# depends on one of them, directly or not.
dependents() {
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$REPO/Cargo.toml" |
        jq -r --args '
            [.packages[] | {name, deps: [.dependencies[] | select(.path != null) | .name]}] as $p
            | def closure(s): ([$p[] | select(any(.deps[]; . as $d | s | index($d))) | .name] + s | unique) as $n
                | if ($n | length) == (s | length) then s else closure($n) end;
            closure($ARGS.positional) | .[]' "$@"
}

crates=()
if [[ -n ${MIRQAB_AFFECTED:-} ]]; then
    # The same BASE and flags tests/run.sh --affected was given.
    read -ra affected_args <<<"$MIRQAB_AFFECTED"
    changed="$(cd "$REPO" && tools/affected-tests.sh --paths "${affected_args[@]}")"
    touched=()
    shared=0
    while IFS= read -r p; do
        case $p in
        crates/mirqab-proto/* | Cargo.toml | Cargo.lock | deny.toml | rust-toolchain.toml) shared=1 ;;
        crates/*/*) touched+=("$(cut -d/ -f2 <<<"$p")") ;;
        esac
    done <<<"$changed"
    if ((shared)); then
        mapfile -t crates < <(members)
    elif ((${#touched[@]})); then
        mapfile -t crates < <(dependents "${touched[@]}" | sort -u)
    fi
else
    mapfile -t crates < <(members)
fi

if ((${#crates[@]} == 0)); then
    ok
else
    args=()
    for c in "${crates[@]}"; do args+=(-p "$c"); done
    printf '  cargo test %s\n' "${crates[*]}"
    capture cargo test --locked --quiet --manifest-path "$REPO/Cargo.toml" "${args[@]}"
    status_is "cargo test ${crates[*]}" 0 "$STATUS"
    ((STATUS == 0)) || printf '%s\n' "$OUTPUT" | tail -40 >&2
fi
