#!/usr/bin/env bash
# Story 19-2 (D4, AC2) — hard-coded V-code gate.
#
# A spec that creates a volume takes its V-code from `specVcode` (helpers/vcode.ts):
# one block per spec file, so two specs never create the same label. A V-code
# literal (`V` + 4 digits) in a spec or helper is allowed only where no volume
# is created with it — refusal text, an example in an error message, a burst
# that must be swallowed — and must be listed in ALLOWED below as
# `file:literal`. Comment lines (`//`, `*`) are ignored.
#
# Usage: tests/e2e/vcode-literals.sh   (from anywhere; exit 1 on an unlisted literal)
set -euo pipefail
cd "$(dirname "$0")"

ALLOWED=(
  # Refused by the client (V0000 is never a valid label).
  "specs/journeys/catalog-volume.spec.ts:V0000"
  # Scanned with no title in context → warning; block 0 is never handed out.
  "specs/journeys/catalog-volume.spec.ts:V0099"
  # Not-found scan on /loans; block 0 is never handed out.
  "specs/journeys/loans.spec.ts:V0199"
  # The example code named in the "invalid V-code" message.
  "specs/journeys/title-volume-add.spec.ts:V0042"
  "specs/journeys/vcode-any-case.spec.ts:V0042"
  # A keystroke burst the open drawer must swallow (nothing is created).
  "specs/journeys/navbar-hamburger.spec.ts:V0001"
)

status=0
while IFS= read -r hit; do
  file=${hit%%:*}
  rest=${hit#*:}
  line=${rest#*:}
  trimmed=${line#"${line%%[![:space:]]*}"}
  case "$trimmed" in
    //* | \** | /\**) continue ;;
  esac
  for code in $(grep -oE '\bV[0-9]{4}\b' <<<"$line" | sort -u); do
    ok=0
    for allowed in "${ALLOWED[@]}"; do
      [[ "$allowed" == "$file:$code" ]] && ok=1 && break
    done
    if [[ $ok -eq 0 ]]; then
      echo "hard-coded V-code $code in $hit" >&2
      echo "  → take it from specVcode() (helpers/vcode.ts), or allow-list it in $0 if no volume is created with it" >&2
      status=1
    fi
  done
done < <(grep -rnE '\bV[0-9]{4}\b' specs helpers --include='*.ts' | grep -v '^helpers/vcode.ts:' || true)

# A V-code built by hand (`V${…}`, `V7${…}`) bypasses the blocks just the same.
while IFS= read -r hit; do
  echo "V-code built by hand in $hit" >&2
  echo "  → take it from specVcode() (helpers/vcode.ts)" >&2
  status=1
done < <(grep -rnE '`V[0-9]*\$\{' specs helpers --include='*.ts' | grep -v '^helpers/vcode.ts:' || true)

exit $status
