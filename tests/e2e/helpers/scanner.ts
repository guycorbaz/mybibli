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
  // When the selector is "body" the caller wants to fire a scanner burst
  // at the document level (e.g. modal-open scanner-guard regression tests).
  // Calling `body.focus()` would steal focus from the actual focused element
  // (Cancel button under a modal) and defeat the test's premise — keep focus
  // wherever it currently is so the burst exercises the real scanner path.
  if (selector !== "body") {
    await page.locator(selector).focus();
  }
  await page.keyboard.type(code, { delay: 20 });
  await page.keyboard.press("Enter");
}

/**
 * Simulate a human typing into the selected field at **100 ms** inter-key —
 * slow enough to cross the `scanner_burst_threshold` so `search.js` and
 * `scan-field.js` classify the input as typing, not a scan.
 *
 * Fix #196 (v1.7.9): was `pressSequentially` which under default-worker
 * parallelism (14 workers on the author's machine) occasionally dropped a
 * keystroke — the trailing `t` in `"test"` would never reach the field and
 * the URL assertion saw `q=tes` instead of `q=test`. `keyboard.type` after
 * an explicit `focus()` matches the simulateScan pattern (battle-tested
 * across the suite) and does not re-focus between keys, eliminating the
 * race. The 6-retro-old flake stays closed in CI at 2 workers AND on a
 * local default-worker run.
 */
export async function simulateTyping(
  page: Page,
  selector: string,
  text: string,
): Promise<void> {
  await page.locator(selector).focus();
  await page.keyboard.type(text, { delay: 100 });
}
