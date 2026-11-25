/**
 * Story 18-5 (#526) — an error on the series page is shown, not dropped.
 *
 * Every AppError retargets to `#feedback-list`; the series page had none,
 * so a failed HTMX action did nothing visible. Since 18-5 the base layout
 * carries a default region. The reported case: a series deleted in
 * another tab, then "Delete" clicked on the stale page — the delete modal
 * request answers "not found", which must land next to the page content,
 * with no dialog opened and focus left on the button (Rule #21). And it
 * must be the server's message: HTMX drops 4xx bodies, so mybibli.js's
 * responseError handler inserts an AppError answer (HX-Retarget
 * #feedback-list + one feedback entry); before that, only the generic
 * "Server error (404)" line showed.
 *
 * From a blank browser; the spec uses its own series (Rule #23).
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";
import { expectHtmxProcessed } from "../../helpers/htmx";
import { backgroundAlpha, expectReallySeen } from "../../helpers/visibility";

const NOT_FOUND = /couldn't find what you were looking for|Introuvable|nicht finden|non .*trovat/i;
const TOME_GONE = /expected tome no longer exists|tome attendu n'existe plus/i;
const DELETE = /^(Delete|Supprimer|Löschen|Elimina)$/i;
// mybibli.js's fallback for an error it could not show (#526: until 18-5
// every AppError body was dropped by HTMX and this line stood in for it).
const GENERIC = /Server error|Erreur serveur|Serverfehler|Errore del server/i;

test.describe("Story 18-5 — errors visible on the series page (#526)", () => {
  test("a series deleted in another tab: Delete shows not found, focus stays", async ({
    page,
    context,
  }) => {
    await context.clearCookies();
    await loginAs(page, "librarian");

    const name = `SF-Gone-${Date.now()}`;
    await page.goto("/series/new");
    await page.locator("#series-name").fill(name);
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+$/);
    const seriesUrl = page.url();

    // The region exists and is empty before anything fails.
    const region = page.locator("#feedback-list");
    await expect(region).toHaveCount(1);
    await expect(region).toHaveAttribute("aria-live", "polite");
    await expect(region.locator(".feedback-entry")).toHaveCount(0);

    // Another tab deletes the series.
    const other = await context.newPage();
    await other.goto(seriesUrl);
    await other.getByRole("button", { name: DELETE }).click();
    await expect(other.locator("#modal-slot dialog[open]")).toBeVisible();
    await other.locator("[data-modal-confirm]").click();
    await other.waitForURL(/\/series$/);
    await other.close();

    // Back on the stale page: Delete asks for a modal of a series that is gone.
    const deleteBtn = page.getByRole("button", { name: DELETE });
    await deleteBtn.click();
    await expect(region.locator(".feedback-entry")).toContainText(NOT_FOUND, {
      timeout: 10000,
    });
    // The server's localized message, not the generic "Server error (404)".
    await expect(region).not.toContainText(GENERIC);
    await expect(region.locator(".feedback-entry")).toHaveCount(1);
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
    await expect(deleteBtn).toBeFocused();

    await checkAccessibility(page);
  });

  // Story 19-4 (#689 item 1) — an error answered to a control low on a long
  // page lands in the region at the top: it is scrolled into view, and
  // focus stays on the control (Rule #21). Also #712: the UX-DR8 modal's
  // backdrop dims the page (alpha 0.5), it does not hide it.
  test("an error below the fold is brought into view, focus stays on the control", async ({
    page,
    context,
  }) => {
    await context.clearCookies();
    await loginAs(page, "librarian");

    // A closed series of 80: its grid runs far below the fold, and the
    // expected tome sits in the last square.
    const name = `SF-Long-${Date.now()}`;
    await page.goto("/series/new");
    await page.locator("#series-name").fill(name);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("80");
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+$/);
    const seriesUrl = page.url();
    await page.goto(`${new URL(seriesUrl).pathname}/expected-tomes/new`);
    await page.locator("#expected-tome-position").fill("80");
    await page.locator("#expected-tome-title").fill(`${name} tome`);
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL((url) => url.pathname === new URL(seriesUrl).pathname);

    // Another tab deletes the tome, through the UX-DR8 modal.
    const other = await context.newPage();
    await other.goto(seriesUrl);
    await other
      .locator("[data-expected-tome]")
      .filter({ hasText: `${name} tome` })
      .getByRole("button", { name: /^(delete|supprimer) —/i })
      .click();
    const dialog = other.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible();
    expect(await backgroundAlpha(dialog)).toBeCloseTo(0.5, 2);
    const confirm = other.locator("#modal-slot dialog[open] [data-modal-confirm]");
    await expectHtmxProcessed(confirm);
    await confirm.click();
    await expect(other.locator("[data-expected-tome]")).toHaveCount(0);
    await other.close();

    // Back on the stale page, far down: Delete asks for the modal of a tome
    // that is gone; the answer lands in the region at the top.
    const del = page
      .locator("[data-expected-tome]")
      .filter({ hasText: `${name} tome` })
      .getByRole("button", { name: /^(delete|supprimer) —/i });
    await del.scrollIntoViewIfNeeded();
    await expect(page.locator("h1")).not.toBeInViewport();
    await del.click();
    const entry = page.locator("#feedback-list .feedback-entry");
    await expect(entry).toContainText(TOME_GONE, { timeout: 10000 });
    await expect(entry).toHaveAttribute("data-feedback-variant", "error");
    await expectReallySeen(entry, { inViewport: true });
    await expect(del).toBeFocused();
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
  });
  test("an error entry below the viewport does not scroll the page (story 19-4 review)", async ({
    page,
  }) => {
    // On /catalog an older entry low in a long list can be updated late
    // (an OOB metadata warning): the page must not jump while scanning.
    // Only an entry above the viewport is brought into view.
    await loginAs(page, "librarian");
    await page.goto("/series");
    await page.evaluate(() => {
      const list = document.getElementById("feedback-list");
      if (!list) throw new Error("no #feedback-list");
      for (let i = 0; i < 60; i++) {
        const filler = document.createElement("p");
        filler.className = "feedback-entry p-3";
        filler.setAttribute("data-feedback-variant", "info");
        filler.textContent = `filler ${i}`;
        list.appendChild(filler);
      }
      window.scrollTo(0, 0);
    });
    await page.evaluate(
      () =>
        new Promise<void>((resolve) => {
          const list = document.getElementById("feedback-list")!;
          const late = document.createElement("div");
          late.id = "sf-late-warning";
          late.className = "feedback-entry p-3";
          late.setAttribute("data-feedback-variant", "warning");
          late.textContent = "late warning";
          list.appendChild(late);
          // The observer runs as a microtask; two frames let any scroll start.
          requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
        }),
    );
    await expect(page.locator("#sf-late-warning")).not.toBeInViewport();
    expect(await page.evaluate(() => window.scrollY)).toBe(0);
  });
});
