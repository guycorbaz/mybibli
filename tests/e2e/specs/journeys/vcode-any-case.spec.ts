/**
 * Story 17-1 E2E (#651, FR172) — a V-code is recognised whatever its case.
 *
 * Real journey from a blank browser (the reported case): loginAs(librarian) →
 * /catalog → New title → a title created by hand, without ISBN → a V-code
 * typed in lowercase adds its volume under the canonical `V` label → the title
 * page lists it. A near V-code is refused naming the expected shape (not the
 * media-type message), and an unrecognised code names the accepted shapes;
 * focus stays in the scan field throughout (Rule #21).
 *
 * Data: the title name and the V-codes are this spec's own (range V9500–V9899,
 * random); assertions only on the rows this test creates (Rule #23).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { simulateScan, simulateTyping } from "../../helpers/scanner";

async function createTitleByHand(page: Page, name: string): Promise<void> {
  await page.locator("#new-title-btn").click();
  const form = page.locator("#title-form-container form");
  await expect(form).toBeVisible({ timeout: 10000 });
  await form.locator("#title-field").fill(name);
  await form.locator("#media-type-field").selectOption("book");
  await page.waitForSelector("#page-count-field", { timeout: 10000 });
  await form.locator("#page-count-field").fill("48");
  await form.locator("#genre-field").selectOption({ index: 1 });
  await form.locator("#language-field").fill("fr");
  await form.locator('button[type="submit"]').click();
  await expect(
    page.locator('.feedback-entry[data-feedback-variant="success"]').first(),
  ).toBeVisible({ timeout: 10000 });
  await expect(form).not.toBeVisible();
}

test.describe("Story 17-1 — V-code any case", () => {
  test("a hand-made title gets its volume from a lowercase V-code", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const name = `La planète de l'angoisse VK ${Date.now()}`;
    await createTitleByHand(page, name);

    const digits = String(9500 + Math.floor(Math.random() * 400));
    const label = `V${digits}`;
    await simulateTyping(page, "#scan-field", `v${digits}`);
    await page.keyboard.press("Enter");

    const created = page.locator('.feedback-entry[data-feedback-variant="success"]').filter({ hasText: label });
    await expect(created.first()).toBeVisible({ timeout: 10000 });
    await expect(page.locator("#scan-field")).toBeFocused();

    // A near V-code names the expected shape, not media types.
    await simulateScan(page, "#scan-field", "V12A4");
    const shape = page.locator(".feedback-entry").filter({ hasText: /V0042/ });
    await expect(shape.first()).toBeVisible({ timeout: 10000 });
    await expect(page.locator("#feedback-list")).not.toContainText(/disambiguation|distinction de type/i);
    await expect(page.locator("#scan-field")).toBeFocused();

    // The reported input: five digits, no V — the accepted shapes are named.
    await simulateScan(page, "#scan-field", "11338");
    const unknown = page.locator(".feedback-entry").filter({ hasText: /not recognised|pas reconnu/i });
    await expect(unknown.first()).toBeVisible({ timeout: 10000 });
    await expect(unknown.first()).toContainText(/V0042/);
    await expect(page.locator("#scan-field")).toBeFocused();

    // The home scan reaches the volume from the lowercase code too; its page
    // shows the canonical label and the hand-made title.
    const volumeUrl = await page.evaluate(async (code) => {
      const res = await fetch(`/scan?code=${code}`, { headers: { "HX-Request": "true" } });
      return res.headers.get("hx-redirect");
    }, `v${digits}`);
    expect(volumeUrl).toMatch(/^\/volume\/\d+$/);
    await page.goto(volumeUrl!);
    await expect(page.locator("main")).toContainText(label);
    await expect(page.locator("main")).toContainText(name);
  });
});
