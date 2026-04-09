import { Page, expect } from "@playwright/test";

/**
 * Create a root location with a specific name and L-code.
 * Uses the /locations form directly. Returns the L-code.
 */
export async function createLocation(
  page: Page,
  name: string,
  lcode: string,
): Promise<string> {
  await page.goto("/locations");
  await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
  await page.locator("#new-name").fill(name);
  // Override the auto-proposed L-code with our unique one
  await page.locator("#new-lcode").fill(lcode);
  await page.locator('button[type="submit"]').last().click();
  await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
  await expect(page.locator(`text=${name}`)).toBeVisible({ timeout: 5000 });
  return lcode;
}
