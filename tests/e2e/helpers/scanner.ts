import { Page } from "@playwright/test";

/**
 * Simulate a USB barcode scanner keyboard-wedge burst into the selected
 * field, followed by the Enter suffix scanners always append.
 *
 * Inter-key delay is **20 ms**, well below the server-side
 * `scanner_burst_threshold` default of 50 ms (see `src/config.rs`) and
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
 * Inter-key delay of {@link simulateTyping}. Story 19-2 (#674): it was
 * 100 ms, exactly the home search field's hard-coded
 * `data-scanner-threshold="100"` (`templates/pages/home.html`), while
 * `static/js/search.js` treats only an inter-key time *strictly above* the
 * threshold as typing — no margin on a loaded runner. (The server-side
 * `scanner_burst_threshold` default is 50 ms, `src/config.rs`.)
 */
export const TYPING_DELAY_MS = 150;

/** Margin required between {@link TYPING_DELAY_MS} and the field's threshold. */
const TYPING_MARGIN_MS = 30;

/**
 * Simulate a human typing into the selected field at
 * {@link TYPING_DELAY_MS} inter-key — slow enough to cross the field's
 * scanner threshold so `search.js` and `scan-field.js` classify the input as
 * typing, not a scan. Throws when the field's threshold (its own
 * `data-scanner-threshold`, else `<body>`'s) leaves less than
 * {@link TYPING_MARGIN_MS} of margin, rather than letting a spec go flaky.
 *
 * Fix #196 (v1.7.9): was `pressSequentially` which under default-worker
 * parallelism (14 workers on the author's machine) occasionally dropped a
 * keystroke — the trailing `t` in `"test"` would never reach the field and
 * the URL assertion saw `q=tes` instead of `q=test`. `keyboard.type` after
 * an explicit `focus()` matches the simulateScan pattern (battle-tested
 * across the suite) and does not re-focus between keys, eliminating the
 * race.
 */
export async function simulateTyping(
  page: Page,
  selector: string,
  text: string,
): Promise<void> {
  const field = page.locator(selector);
  const threshold = await field.evaluate((el) => {
    const own = el.getAttribute("data-scanner-threshold");
    const body = document.body.getAttribute("data-scanner-threshold");
    return parseInt(own || body || "", 10);
  });
  if (!Number.isFinite(threshold)) {
    throw new Error("simulateTyping: no readable data-scanner-threshold on the field or <body>");
  }
  if (TYPING_DELAY_MS < threshold + TYPING_MARGIN_MS) {
    throw new Error(
      `simulateTyping: ${TYPING_DELAY_MS} ms per key is within ${TYPING_MARGIN_MS} ms of the field's scanner threshold (${threshold} ms) — typing would be read as a scan`,
    );
  }
  await field.focus();
  await page.keyboard.type(text, { delay: TYPING_DELAY_MS });
}

/**
 * #574 — `scan-field.js` drops a code identical to the previous one posted
 * less than `DUPLICATE_SCAN_WINDOW_MS` (1000 ms) earlier: a scanner double
 * read. A spec that deliberately re-scans the same code (a librarian
 * scanning a book again) must first let that window elapse. Waits on the
 * `data-last-scan-at` stamp the field carries — no `waitForTimeout`.
 */
export const DUPLICATE_SCAN_WINDOW_MS = 1000;

export async function waitForDuplicateScanWindow(
  page: Page,
  selector = "[data-mybibli-scan-field]",
): Promise<void> {
  await page.waitForFunction(
    ({ sel, windowMs }) => {
      const field = document.querySelector(sel);
      const at = Number(field?.getAttribute("data-last-scan-at") ?? 0);
      return Date.now() - at >= windowMs;
    },
    { sel: selector, windowMs: DUPLICATE_SCAN_WINDOW_MS },
    { polling: 100 },
  );
}
