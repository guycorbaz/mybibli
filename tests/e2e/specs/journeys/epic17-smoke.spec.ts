/**
 * Story 17-7 — Epic 17 smoke test (Foundation Rule #7): everyday entry, end
 * to end, from a blank browser. If this test fails, epic 17 is not done.
 *
 * Journey:
 *   loginAs(admin) → /catalog: a title created by hand with a hyphenated
 *   ISBN (17-3) → its page, reached through the search on its name → a
 *   lowercase V-code scanned into the volumes section adds the volume to
 *   this title (17-1, 17-2) → the bare ISBN scanned on /catalog lands on
 *   that same title, no duplicate (17-3) → `#<id>` in the search finds it
 *   (17-5) → Admin → Skills shows a default skill, inactive (17-6).
 *
 * Data: ISBN from specIsbn("E7", random seq) — manual creation refuses an
 * ISBN another title holds, so neither a rerun on a database not reset nor
 * a parallel repeat may reuse a fixed one; the title name carries the spec id and a timestamp;
 * V-code band V5000–V5499, random (home.spec.ts and loans.spec.ts draw
 * widely, so a collision is rare but possible). Assertions only on this
 * spec's rows (Rule #23); the default skill is a row every database has,
 * read only, asserted by name.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { simulateScan, simulateTyping } from "../../helpers/scanner";

/** Random over the whole `specIsbn` range: a collision is ~1 in 100 000. */
function seq(): number {
  return Math.floor(Math.random() * 100000);
}

function hyphenated(isbn13: string): string {
  return `${isbn13.slice(0, 3)}-${isbn13.slice(3, 5)}-${isbn13.slice(5, 8)}-${isbn13.slice(8, 12)}-${isbn13.slice(12)}`;
}

test.describe("Epic 17 smoke — everyday entry", () => {
  test("a hand-made title gets a volume on its page, is found by scan and by id", async ({ page }) => {
    test.setTimeout(90000);
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // A title created by hand, its ISBN typed with hyphens.
    const isbn13 = specIsbn("E7", seq());
    const name = `Everyday entry E7 ${Date.now()}`;
    await page.goto("/catalog");
    await page.locator("#new-title-btn").click();
    const form = page.locator("#title-form-container form");
    await expect(form).toBeVisible({ timeout: 10000 });
    await form.locator("#title-field").fill(name);
    await form.locator("#media-type-field").selectOption("book");
    await page.waitForSelector("#page-count-field", { timeout: 10000 });
    await form.locator("#genre-field").selectOption({ index: 1 });
    await form.locator("#language-field").fill("fr");
    await form.locator("#isbn-field").fill(hyphenated(isbn13));
    await form.locator('button[type="submit"]').click();
    await expect(
      page.locator('.feedback-entry[data-feedback-variant="success"]').first(),
    ).toBeVisible({ timeout: 10000 });
    await expect(form).not.toBeVisible();

    // Its page, reached the way a librarian does: the search on its name.
    await page.goto(`/?q=${encodeURIComponent(name)}`);
    const hit = page
      .locator("#browse-results table.browse-table tbody a[href^='/title/']")
      .filter({ hasText: name });
    await expect(hit).toHaveCount(1, { timeout: 10000 });
    await hit.click();
    await expect(page).toHaveURL(/\/title\/\d+$/);
    const titleId = /\/title\/(\d+)$/.exec(page.url())![1]!;
    await expect(page.locator("#title-id")).toContainText(`#${titleId}`);

    // A lowercase V-code scanned on the title page adds the volume here.
    const digits = String(5000 + Math.floor(Math.random() * 500));
    const label = `V${digits}`;
    const vcodeField = page.locator("#title-volume-vcode");
    await expect(page.locator("#title-volumes-count")).toHaveText("(0)");
    await simulateScan(page, "#title-volume-vcode", `v${digits}`);
    await expect(
      page.locator('#title-volume-add-feedback .feedback-entry[data-feedback-variant="success"]'),
    ).toContainText(label, { timeout: 10000 });
    await expect(page.locator("#title-volumes-list")).toContainText(label);
    await expect(page.locator("#title-volumes-count")).toHaveText("(1)");
    await expect(vcodeField).toBeFocused();

    // The bare ISBN scanned on /catalog lands on that same title.
    await page.goto("/catalog");
    await simulateScan(page, "#scan-field", isbn13);
    await expect(
      page
        .locator(".feedback-entry")
        .filter({ hasText: /already in your catalog|déjà dans votre catalogue/i })
        .first(),
    ).toBeVisible({ timeout: 10000 });
    await expect(page.locator("#context-banner")).toContainText(name, { timeout: 10000 });

    // Its id in the search lists that title alone and opens it.
    await page.goto("/");
    await simulateTyping(page, "#search-field", `#${titleId}`);
    const rows = page.locator("#browse-results table.browse-table tbody tr");
    const byId = rows.locator(`a[href="/title/${titleId}"]`).first();
    await expect(byId).toBeVisible({ timeout: 10000 });
    await expect(rows).toHaveCount(1);
    await byId.click();
    await expect(page).toHaveURL(new RegExp(`/title/${titleId}$`));
    await expect(page.locator("#title-volumes-list")).toContainText(label);

    // A default skill ships inactive, as a model.
    await page.goto("/admin", { waitUntil: "domcontentloaded" });
    await page.getByRole("tab", { name: /^(Skills|Compétences|Skill)$/i }).click();
    await expect(page.locator("#admin-skills-editor")).toBeVisible({ timeout: 10000 });
    const skill = page
      .locator("#admin-skills-list tbody tr")
      .filter({ has: page.getByText("comics-covers", { exact: true }) });
    await expect(skill).toHaveCount(1);
    await expect(skill.getByText(/^(Inactive|Inaktiv|Inattivo)$/i)).toBeVisible();
    await expect(
      skill.getByText(/^(provided by mybibli|fourni par mybibli|von mybibli bereitgestellt|fornito da mybibli)$/),
    ).toBeVisible();
  });
});
