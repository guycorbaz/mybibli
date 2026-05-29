import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";

// CR #350 — make the manual cover-upload affordance more visible (FR/CH/DE
// publisher gap pivot). A title scanned with a generated ISBN resolves to
// synthetic BnF metadata with NO cover (and the Open Library cover fallback
// 404s), so it is deterministically cover-less even after the async fetch.

const UPLOAD_BTN = /Upload cover|Téléverser une couverture|Cover hochladen|Carica una copertina/i;
const EXPLAINER = /Many publishers|Beaucoup d'éditeurs|Viele Verlage|Molti editori/i;
const REVIEW_CTA = /Review titles without a cover|Voir les titres sans couverture|Titel ohne Cover ansehen|Vedi i titoli senza copertina/i;

test.describe("Cover-upload affordance (#350)", () => {
  test("librarian: cover-less title detail shows prominent CTA + explainer; click opens modal", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    const isbn = specIsbn("UC", 1);
    await scanTitleAndVolume(page, isbn, "V0350");

    // ISBN search returns the single matching title in #browse-results. Read
    // its detail href and navigate directly — the card markup carries a
    // desktop + md:hidden mobile twin, so clicking ".first()" can hit the
    // hidden copy. getAttribute works regardless of visibility.
    await page.goto("/?q=" + isbn);
    const detailHref = await page
      .locator('#browse-results a[href^="/title/"]')
      .first()
      .getAttribute("href");
    expect(detailHref).toBeTruthy();
    await page.goto(detailHref!);

    // Cover-less → prominent upload button + honest gap explainer.
    const uploadBtn = page.getByRole("button", { name: UPLOAD_BTN });
    await expect(uploadBtn).toBeVisible();
    await expect(page.getByText(EXPLAINER)).toBeVisible();

    // Clicking the CTA loads the upload modal into #modal-slot (file input).
    await uploadBtn.click();
    await expect(page.locator('#modal-slot input[type="file"]')).toBeVisible();
  });

  test("admin: Health tab links to the no-cover review list when titles remain uncovered", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    // A cover-less title WITH an identifier counts toward the bulk-refetch
    // "missing covers" total, so the review CTA renders.
    const isbn = specIsbn("UC", 2);
    await scanTitleAndVolume(page, isbn, "V0351");

    await page.goto("/admin?tab=health");
    const reviewLink = page.getByRole("link", { name: REVIEW_CTA });
    await expect(reviewLink).toBeVisible();
    await expect(reviewLink).toHaveAttribute("href", "/?filter=no_cover");
  });
});
