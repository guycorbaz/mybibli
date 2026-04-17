import { Page } from "@playwright/test";

/**
 * Simulate a USB barcode scanner keyboard-wedge burst into the selected
 * field, followed by the Enter suffix scanners always append.
 *
 * Inter-key delay is **20 ms**, well below the server-side
 * `scanner_burst_threshold` default of 100 ms (see `src/config.rs`) and
 * matching the USB-HID envelope used by `static/js/search.js` to classify
 * bursts as scans rather than human typing.
 *
 * Uses Playwright's native `{ delay }` option — do NOT replace this with
 * manual `keyboard.down/up` sequences spaced by `waitForTimeout`, which
 * would trip the CI grep gate in `tests/e2e/helpers/`.
 */
export async function simulateScan(
  page: Page,
  selector: string,
  code: string,
): Promise<void> {
  await page.locator(selector).focus();
  await page.keyboard.type(code, { delay: 20 });
  await page.keyboard.press("Enter");
}

/**
 * Simulate a human typing into the selected field at **100 ms** inter-key —
 * slow enough to cross the `scanner_burst_threshold` so `search.js` and
 * `scan-field.js` classify the input as typing, not a scan.
 */
export async function simulateTyping(
  page: Page,
  selector: string,
  text: string,
): Promise<void> {
  await page.locator(selector).pressSequentially(text, { delay: 100 });
}
