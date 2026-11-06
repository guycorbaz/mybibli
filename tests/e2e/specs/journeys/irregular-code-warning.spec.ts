/**
 * Story 16-5 E2E (#638, FR170, UX-DR42) — a scanned code that is no known
 * barcode shape is flagged; nothing is created until the librarian chooses.
 *
 * Real journey from a blank browser: loginAs(librarian) → a 9-digit code
 * shows the choice with its warning, focus stays in the scan field, no title
 * exists → the librarian corrects it: a valid barcode shows the choice with no
 * warning → an irregular code confirmed keeps its digits as entered → with the
 * media-type preference now set, a further irregular code still asks.
 *
 * Spec ID: random codes of this spec's own shapes; assertions only on rows
 * this test creates (Rule #23).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/** `length` random digits, never starting like an ISBN or an ISSN prefix. */
function digits(length: number): string {
  let out = "5";
  while (out.length < length) out += Math.floor(Math.random() * 10);
  return out;
}

/** 10 digits that fail as an ISBN-10 (a valid one would be read as an ISBN). */
function badIsbn10(): string {
  const body = digits(9);
  const sum = [...body].reduce((acc, c, i) => acc + Number(c) * (10 - i), 0);
  const valid = (11 - (sum % 11)) % 11;
  return body + String((valid + 1) % 10 === valid ? (valid + 2) % 10 : (valid + 1) % 10);
}

/** A valid EAN-13 of this spec's own. */
function validEan13(): string {
  const body = digits(12);
  const sum = [...body].reduce((acc, c, i) => acc + Number(c) * (i % 2 === 0 ? 1 : 3), 0);
  return body + String((10 - (sum % 10)) % 10);
}

async function scan(page: Page, code: string) {
  const field = page.locator("#scan-field");
  await field.fill(code);
  await field.press("Enter");
}

async function titleExists(page: Page, code: string): Promise<boolean> {
  // The search fragment only: the full home page also lists recent titles.
  const response = await page.request.get(`/?q=${code}`, { headers: { "HX-Request": "true" } });
  return (await response.text()).includes(`href="/title/`);
}

test.describe("Story 16-5 — irregular code warning", () => {
  test("an irregular code asks first, and is kept as entered when confirmed", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const nine = digits(9);
    await scan(page, nine);
    const warning = page.locator("[data-code-warning]").last();
    await expect(warning).toContainText(/9/);
    await expect(page.locator("#scan-field")).toBeFocused();
    expect(await titleExists(page, nine)).toBe(false);

    // Corrected: a valid barcode shows the choice with no warning.
    const before = await page.locator("[data-code-warning]").count();
    const valid = validEan13();
    await scan(page, valid);
    await expect(page.getByText(new RegExp(`UPC ${valid}`))).toBeVisible();
    await expect(page.locator("[data-code-warning]")).toHaveCount(before);

    // Confirmed: created under the digits as entered.
    const ten = badIsbn10();
    await scan(page, ten);
    await expect(page.locator("[data-code-warning]")).toHaveCount(before + 1);
    await page.locator(`button[role="radio"][hx-vals*="${ten}"]`, { hasText: /Book|Livre/ }).click();
    await expect(page.locator(".feedback-skeleton").last()).toBeVisible();
    await expect.poll(() => titleExists(page, ten)).toBe(true);

    // The preference is now set: an irregular code still asks.
    const other = digits(11);
    await scan(page, other);
    await expect(page.locator(`button[role="radio"][hx-vals*="${other}"]`).first()).toBeVisible();
    expect(await titleExists(page, other)).toBe(false);
  });
});
