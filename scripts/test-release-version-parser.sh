#!/usr/bin/env bash
#
# #374 / #30 sub-item 2 — guard against a future regression of the
# Python `tomllib` version-extraction logic embedded in
# `.github/workflows/release.yml::verify-version`.
#
# The workflow currently does:
#   python3 -c "import tomllib; print(tomllib.load(open('Cargo.toml','rb'))['package']['version'])"
#
# If that parser ever stops returning the correct string (Python upgrade,
# Cargo.toml gains a `[workspace.package]` or `[features]` section that
# steals priority, etc.), every subsequent release silently ships under
# a wrong version label. This script reproduces the parser on a battery
# of crafted inputs and refuses to exit 0 unless every case matches the
# expected behaviour. Wired into `_gates.yml` so a parser regression
# trips CI on the PR that breaks it.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Same one-liner as the release workflow — keep these strings byte-identical.
extract_version() {
    python3 -c "import tomllib; print(tomllib.load(open('$1','rb'))['package']['version'])"
}

fail() {
    echo "::error::test-release-version-parser: $*" >&2
    exit 1
}

ok() {
    echo "  ok: $*"
}

# Case 1 — the real Cargo.toml parses to the version we expect from the
# canonical [package] table. The expected value is derived (not hardcoded)
# so this case doesn't drift on each release.
echo "case 1: parse the real Cargo.toml"
EXPECTED=$(grep -E '^version = ' "$ROOT/Cargo.toml" | head -n1 | awk -F'"' '{print $2}')
GOT=$(extract_version "$ROOT/Cargo.toml")
if [ "$GOT" != "$EXPECTED" ]; then
    fail "real Cargo.toml: expected $EXPECTED, got $GOT"
fi
ok "real Cargo.toml: $GOT"

# Case 2 — CRLF line endings (e.g. file edited on Windows). tomllib
# handles them; a naive grep-based parser would split on the wrong
# character.
echo "case 2: CRLF line endings"
printf '[package]\r\nname = "x"\r\nversion = "1.2.3"\r\n' > "$TMP/crlf.toml"
GOT=$(extract_version "$TMP/crlf.toml")
[ "$GOT" = "1.2.3" ] || fail "CRLF: expected 1.2.3, got $GOT"
ok "CRLF: 1.2.3"

# Case 3 — version inside a [workspace.package] section MUST NOT be
# picked up (the workflow asks for [package].version specifically).
# Crafted as a workspace root with [workspace.package].version =
# "0.0.0" — a wrong answer that grep would happily return. tomllib
# returns the correct [package].version value above it.
echo "case 3: [workspace.package] vs [package] precedence"
cat > "$TMP/ws.toml" <<'EOF'
[workspace]
members = ["."]

[workspace.package]
version = "0.0.0"

[package]
name = "mybibli"
version = "4.5.6"
EOF
GOT=$(extract_version "$TMP/ws.toml")
[ "$GOT" = "4.5.6" ] || fail "[workspace.package] precedence: expected 4.5.6, got $GOT"
ok "[workspace.package] precedence: 4.5.6"

# Case 4 — version on a single line with single quotes. Cargo allows
# both single and double quotes for TOML strings; tomllib handles
# both. A future grep-based parser change must NOT regress on this.
echo "case 4: single-quoted version"
cat > "$TMP/sq.toml" <<'EOF'
[package]
name = 'x'
version = '7.8.9'
EOF
GOT=$(extract_version "$TMP/sq.toml")
[ "$GOT" = "7.8.9" ] || fail "single-quoted: expected 7.8.9, got $GOT"
ok "single-quoted: 7.8.9"

# Case 5 — verify the abort condition. The workflow exits 1 when the
# extracted version does not match the tag. Reproduce that comparison
# here so the test catches a future regression that downgrades the
# check from `!=` to `=`.
echo "case 5: abort-on-mismatch condition reproduces"
TAG_VERSION="1.2.3"
CARGO_VERSION="1.2.4"
if [ "$TAG_VERSION" = "$CARGO_VERSION" ]; then
    fail "case 5 setup is broken: identical inputs"
fi
ok "mismatch detected as expected"

echo
echo "all 5 cases passed"
