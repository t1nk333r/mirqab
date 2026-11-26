# tests/test-check-docs.sh — tools/check-docs.sh on a scratch copy of the
# record: where git cannot list the Markdown files it says so and exits 2,
# never "check-docs OK" with no link checked (plan 024 L37); in a checkout
# a broken relative link fails it, and a fixed one passes.
# mirqab:covers tools/check-docs.sh

sandbox

docs="$SANDBOX/docs"
mkdir -p "$docs/tools" "$docs/plans"
cp "$REPO/tools/check-docs.sh" "$docs/tools/"
printf '# Plans\n\n| Plan | Title |\n|---|---|\n| 001 | [x](001-x.md) |\n' >"$docs/plans/README.md"
printf '# Plan 001\n' >"$docs/plans/001-x.md"
printf 'See [the plan](plans/001-x.md) and [a gone one](plans/002-gone.md).\n' >"$docs/README.md"

# No repository above the scratch tree: git refuses to list anything.
capture env GIT_CEILING_DIRECTORIES="$SANDBOX" "$docs/tools/check-docs.sh"
status_is "outside a git checkout it cannot check: exit 2" 2 "$STATUS"
like "and says git could not list the files" "$OUTPUT" "git cannot list the Markdown files"
unlike "it never reports OK" "$OUTPUT" "check-docs OK"

git -C "$docs" init -q -b main
capture "$docs/tools/check-docs.sh"
status_is "a broken link fails" 1 "$STATUS"
like "and is named" "$OUTPUT" "README.md:1: broken link: plans/002-gone.md"

printf 'See [the plan](plans/001-x.md).\n' >"$docs/README.md"
capture "$docs/tools/check-docs.sh"
status_is "with the link fixed it passes" 0 "$STATUS"
like "and says so" "$OUTPUT" "check-docs OK"
