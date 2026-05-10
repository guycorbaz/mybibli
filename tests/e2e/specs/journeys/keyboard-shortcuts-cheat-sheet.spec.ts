/**
 * Story 9-20 — keyboard shortcuts + cheat-sheet dialog E2E.
 *
 * Covers AC11 — 5 scenarios:
 *   1. Anonymous opens cheat sheet via `?` (verifies minimal content)
 *   2. Librarian extended cheat sheet
 *   3. g-chord navigation (g then c → /catalog)
 *   4. Input-skip — `?` typed in a search box stays as text
 *   5. Escape closes the dialog (native cancel event)
 *
 * Spec ID "KS" — no ISBNs generated.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

test.describe("Story 9-20 — keyboard shortcuts + cheat sheet", () => {
  test("anonymous opens cheat sheet via `?` and sees minimal content", async ({
    page,
  }) => {
    await page.goto("/");

    // Move focus off any auto-focused input so `?` lands on document
    await page.evaluate(() => {
      const active = document.activeElement;
      if (active && active !== document.body) (active as HTMLElement).blur();
    });

    await page.keyboard.press("?");

    const dialog = page.locator("dialog#shortcuts-cheat-sheet");
    await expect(dialog).toHaveAttribute("open", "");
    await expect(dialog).toContainText(/Keyboard shortcuts|Raccourcis clavier/i);
    // Anonymous: g-h, g-c visible
    await expect(dialog).toContainText(/Go to home|Aller à l'accueil/i);
    await expect(dialog).toContainText(/Go to catalog|Aller au catalogue/i);
    // Anonymous: librarian-only shortcuts NOT visible
    await expect(dialog).not.toContainText(/Go to loans|Aller aux prêts/i);
  });

  test("librarian sees extended set including g-l and Ctrl+K", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/loans");

    await page.evaluate(() => {
      const active = document.activeElement;
      if (active && active !== document.body) (active as HTMLElement).blur();
    });
    await page.keyboard.press("?");

    const dialog = page.locator("dialog#shortcuts-cheat-sheet");
    await expect(dialog).toHaveAttribute("open", "");
    await expect(dialog).toContainText(/Go to loans|Aller aux prêts/i);
    await expect(dialog).toContainText(/Focus the scan field|Focuser le champ de scan/i);
    await expect(dialog).not.toContainText(/Go to admin|Aller à l'administration/i);
  });

  test("g-chord navigation — g then c → /catalog", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    await page.evaluate(() => {
      const active = document.activeElement;
      if (active && active !== document.body) (active as HTMLElement).blur();
    });
    await page.keyboard.press("g");
    await page.keyboard.press("c");
    await page.waitForURL(/\/catalog/);
    expect(page.url()).toContain("/catalog");
  });

  test("input-skip — `?` typed in search field does NOT open dialog", async ({
    page,
  }) => {
    await page.goto("/");
    const searchField = page.locator("#search-field");
    await searchField.focus();
    await page.keyboard.press("?");

    const dialog = page.locator("dialog#shortcuts-cheat-sheet");
    await expect(dialog).not.toHaveAttribute("open", "");
  });

  test("Escape closes the cheat-sheet dialog (native <dialog> cancel)", async ({
    page,
  }) => {
    await page.goto("/");

    await page.evaluate(() => {
      const active = document.activeElement;
      if (active && active !== document.body) (active as HTMLElement).blur();
    });
    await page.keyboard.press("?");

    const dialog = page.locator("dialog#shortcuts-cheat-sheet");
    await expect(dialog).toHaveAttribute("open", "");

    await page.keyboard.press("Escape");
    await expect(dialog).not.toHaveAttribute("open", "");
  });
});
