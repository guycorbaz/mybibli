import { Page } from "@playwright/test";

/**
 * Story 8-2 — fetch the current page's CSRF token from the
 * `<meta name="csrf-token">` tag and return the header object tests
 * should merge into `page.request.post/put/patch/delete` calls.
 *
 * Caller must have navigated to a page that renders `layouts/base.html`
 * (every authenticated page does). Returns `{}` if the meta tag is
 * missing so the request still fires and the test observes the real
 * 403 instead of throwing a TypeError.
 */
export async function csrfHeaders(page: Page): Promise<Record<string, string>> {
  const token = await page
    .locator('meta[name="csrf-token"]')
    .getAttribute("content")
    .catch(() => null);
  if (!token) {
    return {};
  }
  return { "X-CSRF-Token": token };
}
