/**
 * Story 13-8 (#580) — move a volume to another title.
 *
 * A label attached under the wrong title (a publisher EAN shared by several
 * albums) is fixed from the volume page: "Move to another title" opens the
 * UX-DR8 modal, the librarian finds the right title by ISBN (typed as a
 * scanner burst — the scanner guard must let it reach the modal's search
 * field), picks it, confirms; the volume now lives under the target title,
 * on both title pages.
 *
 * Rule #7 style: a blank browser and a real login (loginAs), no injected
 * cookie. Spec ID "MV"; ISBN seq randomised. V-codes come from a clock
 * counter in V0480–V0579 — disjoint from home.spec.ts (`Date.now()`-derived,
 * ≥ V1000), location-full-path.spec.ts (V0600–V0999) and the fixed codes of
 * other specs — so a retry against an un-reset DB starts fresh.
 */
import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { simulateScan } from "../../helpers/scanner";

const DIALOG = "#modal-slot dialog[open]";

/** The /title/:id path of the title holding `isbn`, found by home search. */
async function titlePath(page: Page, isbn: string): Promise<string> {
  await page.goto(`/?q=${isbn}`);
  const link = page
    .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
    .first();
  await expect(link).toBeVisible({ timeout: 15000 });
  return (await link.getAttribute("href"))!;
}

/** Catalogue a title without a volume (a plain ISBN scan). */
async function scanTitleOnly(page: Page, isbn: string) {
  await page.goto("/catalog");
  const field = page.locator("#scan-field");
  await field.fill(isbn);
  await field.press("Enter");
  await expect(page.locator("#context-banner")).toContainText(isbn, {
    timeout: 10000,
  });
}

/** Open the volume page of `label` from its title page. */
async function openVolume(page: Page, titleHref: string, label: string) {
  await page.goto(titleHref);
  await page
    .locator('#title-volumes table a[href^="/volume/"]')
    .filter({ hasText: label })
    .click();
  await expect(page).toHaveURL(/\/volume\/\d+$/);
}

/** `vBase` + a clock counter (0–49): V0480–V0529 or V0530–V0579. */
function seed(vBase: number) {
  const n = 10000 + Math.floor(Math.random() * 89000);
  return {
    isbnFrom: specIsbn("MV", n),
    isbnTo: specIsbn("MV", n + 1),
    label: `V0${vBase + (Date.now() % 50)}`,
  };
}

test.describe("Move a volume to another title (#580)", () => {
  test("a librarian moves a volume through the modal; both title pages follow", async ({
    page,
  }) => {
    const { isbnFrom, isbnTo, label } = seed(480);
    await loginAs(page, "librarian");

    await scanTitleAndVolume(page, isbnFrom, label);
    await scanTitleOnly(page, isbnTo);
    const fromHref = await titlePath(page, isbnFrom);
    const toHref = await titlePath(page, isbnTo);

    await openVolume(page, fromHref, label);
    await expect(page.locator("#volume-title-row a")).toHaveAttribute("href", fromHref);

    await page
      .getByRole("button", { name: /Move to another title|Déplacer vers un autre titre/i })
      .click();
    const dialog = page.locator(DIALOG);
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(label);
    // The search field holds the initial focus.
    await expect(page.locator("#volume-move-search")).toBeFocused();

    // AC4 — axe-core on the open modal.
    const axe = await new AxeBuilder({ page })
      .include(DIALOG)
      .withTags(["wcag2a", "wcag2aa"])
      .analyze();
    expect(axe.violations).toEqual([]);

    // The target ISBN as a scanner burst: the guard forwards it to the
    // modal's search field; the trailing Enter submits nothing.
    await simulateScan(page, "#volume-move-search", isbnTo);
    await expect(page.locator("#volume-move-search")).toHaveValue(isbnTo);
    const candidate = page.locator("#volume-move-candidates [data-move-candidate]");
    await expect(candidate).toHaveCount(1, { timeout: 10000 });
    await expect(candidate).toContainText(isbnTo);
    await expect(dialog).toBeVisible();

    await candidate.click();
    await expect(page.locator("[data-move-confirmation]")).toContainText(label);
    await dialog.locator("[data-modal-confirm]").click();

    await expect(page.locator(DIALOG)).toHaveCount(0);
    await expect(
      page.locator('#feedback-list .feedback-entry[data-feedback-variant="success"]'),
    ).toContainText(label, { timeout: 10000 });
    await expect(page.locator("#volume-title-row a")).toHaveAttribute("href", toHref);

    // Both title pages agree.
    await page.goto(toHref);
    await expect(page.locator("#title-volumes")).toContainText(label);
    await page.goto(fromHref);
    await expect(page.locator("main")).not.toContainText(label);
  });

  test("confirming without a picked title explains why, and the modal stays open", async ({
    page,
  }) => {
    const { isbnFrom, label } = seed(530);
    await loginAs(page, "librarian");
    await scanTitleAndVolume(page, isbnFrom, label);
    const fromHref = await titlePath(page, isbnFrom);
    await openVolume(page, fromHref, label);

    await page
      .getByRole("button", { name: /Move to another title|Déplacer vers un autre titre/i })
      .click();
    const dialog = page.locator(DIALOG);
    await expect(dialog).toBeVisible();
    await dialog.locator("[data-modal-confirm]").click();

    const error = dialog.locator("[data-modal-error]");
    await expect(error).toBeVisible({ timeout: 10000 });
    await expect(error).toContainText(/no title was chosen|aucun titre n’a été choisi/i);
    await expect(dialog).toBeVisible();

    // Cancel leaves the volume where it was.
    await dialog.locator("[data-modal-cancel]").click();
    await expect(page.locator(DIALOG)).toHaveCount(0);
    await expect(page.locator("#volume-title-row a")).toHaveAttribute("href", fromHref);
  });
});
