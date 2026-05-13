# mybibli — User Manual Sources

LaTeX sources for the mybibli end-user manual, produced as one
self-contained PDF per language.

## Layout

```
docs/manual/
├── en/                 # English source — main.tex + chapter files
├── fr/                 # French source — main.tex + chapter files
├── build.sh            # invoke latexmk on each language
└── README.md           # this file
```

Each language directory is **self-contained**: it carries its own
`main.tex`, preamble, chapter files, and `images/` folder. The two
languages share no common LaTeX preamble — drift between EN and FR
preambles is therefore visible at review time, by design.

## Prerequisites

The build uses **XeLaTeX** (for Unicode and system fonts via
`fontspec`) plus **latexmk** to manage multi-pass compilation
(table of contents, index, cross-references):

| Distribution | One-line install |
|--------------|------------------|
| Debian / Ubuntu | `sudo apt-get install texlive-xetex texlive-latex-extra texlive-fonts-recommended texlive-lang-french latexmk` |
| Fedora | `sudo dnf install texlive-scheme-medium texlive-collection-langfrench latexmk` |
| Arch | `sudo pacman -S texlive-most texlive-langfrench` |
| macOS | `brew install --cask mactex-no-gui` (full) or `basictex` (then `tlmgr install latexmk fontspec polyglossia hyphen-french`) |

## Building

```bash
./build.sh            # both languages
./build.sh en         # English only
./build.sh fr         # French only
./build.sh clean      # remove .aux/.log/etc, keep PDFs
./build.sh distclean  # remove .aux/.log/etc AND PDFs
```

Output files land at the top of `docs/manual/`:

```
docs/manual/
├── mybibli-manual-en.pdf
└── mybibli-manual-fr.pdf
```

The script fails fast with a clear message when `xelatex`, `latexmk`
or `makeindex` is missing.

## Editing

* **Chapter files** are named with a numeric prefix (`01-`, `02-`, …)
  that controls the reading order. They are pulled into `main.tex`
  via `\input{NN-name}`.
* **Index entries** are added inline with `\index{term}` next to the
  first meaningful mention of a term. The index is built automatically
  during the latexmk run and printed at the end of the document.
* **Cross-references** between chapters use `\label{ch:name}` /
  `\ref{ch:name}` (or `\pageref{}`).
* **Code listings** use the `listings` package; configuration in the
  preamble. Inline shell commands use `\texttt{...}`.
* **Callouts** (Note / Tip / Warning) use the `tcolorbox` environments
  defined in the preamble.

## Updating after a release

When a new version of mybibli is published:

1. Bump the `\date{Version X.Y.Z --- \today}` in both `main.tex` files.
2. Update the relevant chapters (new features, changed env vars, etc.).
3. Run `./build.sh` and review the generated PDFs.
4. Attach the two PDFs to the GitHub Release as build artifacts.

There is no CI build for the manual today — the PDFs are produced
locally by the maintainer at release time. Wiring up a release-tag
GitHub Actions job is tracked as a future enhancement.

## Translation policy

The French version is a **localized adaptation**, not a literal
translation. Idiomatic phrasing, examples and label names follow the
expectations of a Switzerland / France household audience. Keep the
chapter structure aligned across both languages so cross-references
stay valid.
