#!/usr/bin/env bash
# Build the mybibli user manual PDFs (English + French).
#
# Requires: xelatex + latexmk + the standard TeX Live LaTeX packages.
# On Debian/Ubuntu:
#     sudo apt-get install texlive-xetex texlive-latex-extra \
#                          texlive-fonts-recommended texlive-lang-french \
#                          latexmk
# On macOS (with MacTeX or BasicTeX):
#     brew install --cask mactex-no-gui     # full
#     # or:
#     brew install --cask basictex          # minimal — then `tlmgr install ...`
#
# Usage:
#     ./build.sh            # build both languages
#     ./build.sh en         # English only
#     ./build.sh fr         # French only
#     ./build.sh clean      # remove build artifacts (keeps PDFs)
#     ./build.sh distclean  # remove build artifacts AND PDFs

set -euo pipefail

cd "$(dirname "$0")"

REQUIRED_BINS=(xelatex latexmk makeindex)
for bin in "${REQUIRED_BINS[@]}"; do
    if ! command -v "$bin" >/dev/null 2>&1; then
        echo "error: \`$bin\` not found in PATH." >&2
        echo "       Install a TeX Live distribution that includes xelatex," >&2
        echo "       latexmk and makeindex (see the comment at the top of" >&2
        echo "       this script for distribution-specific commands)." >&2
        exit 1
    fi
done

build_lang() {
    local lang="$1"
    local out_pdf="mybibli-manual-${lang}.pdf"

    if [[ ! -d "$lang" ]]; then
        echo "error: directory \`$lang/\` does not exist." >&2
        exit 1
    fi

    echo ">>> Building ${lang} manual..."
    (
        cd "$lang"
        # -xelatex      → use XeLaTeX (needed for fontspec, polyglossia, Unicode)
        # -interaction  → don't stop at minor warnings
        # -halt-on-error → fail fast on real errors
        # -shell-escape → not needed; we use `listings`, not `minted`
        latexmk -xelatex \
                -interaction=nonstopmode \
                -halt-on-error \
                -file-line-error \
                main.tex
    )

    cp "${lang}/main.pdf" "$out_pdf"
    echo ">>> Wrote ${out_pdf}"
}

clean_lang() {
    local lang="$1"
    [[ -d "$lang" ]] || return 0
    echo ">>> Cleaning ${lang} build artifacts..."
    (cd "$lang" && latexmk -C >/dev/null 2>&1 || true)
    # latexmk -C removes main.pdf too; that's fine for clean inside the lang
    # dir. We keep the top-level mybibli-manual-${lang}.pdf untouched.
}

case "${1:-all}" in
    en)    build_lang en ;;
    fr)    build_lang fr ;;
    all)
        build_lang en
        build_lang fr
        echo ""
        echo "Both PDFs built:"
        ls -lh mybibli-manual-*.pdf
        ;;
    clean)
        clean_lang en
        clean_lang fr
        echo "Build artifacts removed (PDFs kept)."
        ;;
    distclean)
        clean_lang en
        clean_lang fr
        rm -f mybibli-manual-en.pdf mybibli-manual-fr.pdf
        echo "Build artifacts and PDFs removed."
        ;;
    *)
        echo "Usage: $0 [en|fr|all|clean|distclean]" >&2
        exit 1
        ;;
esac
