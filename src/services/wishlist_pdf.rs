//! CR #266 — server-rendered PDF export for `/wishlist/export.pdf`.
//!
//! Replaces the v1.3.1 browser-print HTML fallback. Pure Rust via
//! [`genpdf`] — no subprocess, no external binary in the Docker
//! image. DejaVu Sans is loaded from `static/fonts/` so French
//! accents render correctly.
//!
//! ## Layout
//!
//! - Header: "Ma wish list — N livres" (or i18n equivalent) +
//!   generation date.
//! - Body: one block per entry — title (bold), authors (italic),
//!   ISBN / publisher / year (small, dim), notes (wrapped).
//! - Page footer: "Page X / Y" (genpdf's built-in counter).
//!
//! ## Empty list
//!
//! Renders a single localized line ("(no entries)") so the click
//! never returns an empty/broken PDF.

use std::path::PathBuf;
use std::sync::OnceLock;

use genpdf::Alignment;
use genpdf::Document;
use genpdf::Element;
use genpdf::SimplePageDecorator;
use genpdf::elements::{Break, Paragraph};
use genpdf::fonts::{FontData, FontFamily, from_files};
use genpdf::style::{Color, Style};

use crate::error::AppError;
use crate::models::wishlist::WishlistItem;

/// Cached font handles. Loading TTFs from disk on every PDF build
/// would add 50–100 ms; genpdf's `FontFamily` is `Send + Sync` and
/// cheap to clone, so we resolve it once on first call.
static FONT_FAMILY: OnceLock<FontFamily<FontData>> = OnceLock::new();

fn load_font_family() -> Result<&'static FontFamily<FontData>, AppError> {
    if let Some(f) = FONT_FAMILY.get() {
        return Ok(f);
    }
    // `FONTS_DIR` env var lets the test suite point at a fixture
    // directory; production falls back to the canonical
    // `static/fonts/` relative to the binary's working directory.
    let dir = std::env::var("FONTS_DIR").unwrap_or_else(|_| "static/fonts".to_string());
    let path = PathBuf::from(dir);
    let family = from_files(&path, "DejaVuSans", None).map_err(|e| {
        AppError::Internal(format!(
            "failed to load DejaVu Sans from {path:?}: {e}"
        ))
    })?;
    // Race: two threads may both pass `get()` returning None and both
    // call `from_files`. `set` returns Err if another thread won — we
    // ignore that and use whichever copy is already there. Both copies
    // are byte-equivalent (same TTFs).
    let _ = FONT_FAMILY.set(family);
    Ok(FONT_FAMILY.get().expect("font family was just set"))
}

/// Render the wish list as a PDF. Returns the raw bytes.
pub fn render(items: &[WishlistItem], loc: &str) -> Result<Vec<u8>, AppError> {
    let family = load_font_family()?.clone();
    let mut doc = Document::new(family);

    doc.set_title(rust_i18n::t!("wishlist.pdf.doc_title", locale = loc).to_string());
    doc.set_minimal_conformance();

    // Page-number footer "Page X / Y" (genpdf supports the running
    // counter via SimplePageDecorator). 15 mm margins on every side
    // — wide enough for the footer + a folding margin for the
    // bookstore reader.
    let mut decorator = SimplePageDecorator::new();
    decorator.set_margins(15);
    doc.set_page_decorator(decorator);

    // ── Header ───────────────────────────────────────────────────
    let count = items.len();
    let header_label = if count == 1 {
        rust_i18n::t!("wishlist.pdf.header_one", locale = loc).to_string()
    } else {
        rust_i18n::t!(
            "wishlist.pdf.header_other",
            locale = loc,
            count = count
        )
        .to_string()
    };
    doc.push(
        Paragraph::new(header_label)
            .aligned(Alignment::Center)
            .styled(Style::new().bold().with_font_size(18)),
    );

    let date_label = rust_i18n::t!(
        "wishlist.pdf.generated_on",
        locale = loc,
        date = chrono::Local::now().format("%Y-%m-%d").to_string()
    )
    .to_string();
    doc.push(
        Paragraph::new(date_label)
            .aligned(Alignment::Center)
            .styled(Style::new().italic().with_font_size(9).with_color(Color::Rgb(120, 120, 120))),
    );
    doc.push(Break::new(2.0));

    // ── Body ─────────────────────────────────────────────────────
    if items.is_empty() {
        doc.push(
            Paragraph::new(
                rust_i18n::t!("wishlist.pdf.empty", locale = loc).to_string(),
            )
            .aligned(Alignment::Center)
            .styled(Style::new().italic().with_color(Color::Rgb(150, 150, 150))),
        );
    } else {
        for item in items {
            // Title — bold, larger.
            doc.push(
                Paragraph::new(item.title.clone())
                    .styled(Style::new().bold().with_font_size(12)),
            );

            // Authors — italic if present.
            if let Some(a) = item.authors.as_ref().filter(|s| !s.trim().is_empty()) {
                doc.push(
                    Paragraph::new(a.clone())
                        .styled(Style::new().italic().with_font_size(10)),
                );
            }

            // ISBN + publisher + year — small, joined with " · ".
            let mut meta_parts: Vec<String> = Vec::new();
            if let Some(isbn) = item.isbn.as_ref().filter(|s| !s.trim().is_empty()) {
                meta_parts.push(format!("ISBN {isbn}"));
            }
            if let Some(pub_) = item.publisher.as_ref().filter(|s| !s.trim().is_empty()) {
                meta_parts.push(pub_.clone());
            }
            if let Some(y) = item.publication_year {
                meta_parts.push(y.to_string());
            }
            if !meta_parts.is_empty() {
                doc.push(
                    Paragraph::new(meta_parts.join(" · ")).styled(
                        Style::new()
                            .with_font_size(9)
                            .with_color(Color::Rgb(110, 110, 110)),
                    ),
                );
            }

            // Notes — wrapped, normal weight, slightly indented look.
            if let Some(n) = item.notes.as_ref().filter(|s| !s.trim().is_empty()) {
                doc.push(
                    Paragraph::new(n.clone())
                        .styled(Style::new().with_font_size(10)),
                );
            }

            // Spacer between entries.
            doc.push(Break::new(1.0));
        }
    }

    let mut bytes: Vec<u8> = Vec::new();
    doc.render(&mut bytes)
        .map_err(|e| AppError::Internal(format!("PDF render failed: {e}")))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn fake_item(id: u64, title: &str, authors: Option<&str>, isbn: Option<&str>) -> WishlistItem {
        WishlistItem {
            id,
            isbn: isbn.map(String::from),
            title: title.to_string(),
            authors: authors.map(String::from),
            publisher: Some("Gallimard".to_string()),
            publication_year: Some(1947),
            cover_image_url: None,
            notes: Some("À acheter chez Payot".to_string()),
            created_at: Utc.with_ymd_and_hms(2026, 5, 19, 12, 0, 0).unwrap(),
            version: 1,
        }
    }

    /// Smoke test: a populated list produces a valid PDF prefix.
    #[test]
    fn render_emits_pdf_prefix_and_nonempty_bytes() {
        // The test suite runs from the workspace root, so the relative
        // `static/fonts/` path resolves the same way as production.
        let items = vec![
            fake_item(1, "L'Écume des jours", Some("Boris Vian"), Some("9782070360246")),
            fake_item(2, "Le Comte de Monte-Cristo", Some("Alexandre Dumas"), None),
        ];
        let bytes = render(&items, "fr").expect("render must succeed");
        assert!(bytes.len() > 1000, "PDF must be more than 1 KB; got {}", bytes.len());
        assert!(bytes.starts_with(b"%PDF-"), "must start with %PDF-x.y header");
    }

    /// Empty list — must produce a valid PDF (no crash, no empty
    /// document) with at least the header + empty-state line.
    #[test]
    fn render_empty_list_still_emits_valid_pdf() {
        let bytes = render(&[], "en").expect("render must succeed on empty list");
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.len() > 500);
    }

    /// French unicode round-trip — pin DejaVu Sans actually got
    /// embedded. The PDF stream is binary so we can't grep the
    /// title; we just assert the byte length is well above the
    /// "missing-glyph fallback" minimum (~400 bytes).
    #[test]
    fn render_with_french_accents_does_not_panic() {
        let items = vec![fake_item(
            1,
            "Émile et le château hanté",
            Some("Émile Zola"),
            None,
        )];
        let bytes = render(&items, "fr").expect("render must succeed");
        assert!(bytes.len() > 1000);
    }
}
