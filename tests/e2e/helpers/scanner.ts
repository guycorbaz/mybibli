import { Page } from "@playwright/test";

/**
 * Simulate a barcode scanner burst input.
 * Scanners type characters very quickly (< 50ms between keystrokes).
 */
export async function simulateScan(
  page: Page,
  selector: string,
  code: string,
): Promise<void> {
  // Stub — will be implemented when scan field is built
  await page.locator(selector).fill(code);
}

/**
 * Simulate manual typing at human speed.
 */
export async function simulateTyping(
  page: Page,
  selector: string,
  text: string,
): Promise<void> {
  // Stub — will be implemented when scan field is built
  await page.locator(selector).pressSequentially(text, { delay: 100 });
}
