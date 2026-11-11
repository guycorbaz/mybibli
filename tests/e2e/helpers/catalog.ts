/**
 * Catalog test helpers (#22 test-hygiene consolidation).
 *
 * Centralizes the two ID-resolution patterns that several specs previously
 * inlined fragilely: reading the title id from the scan skeleton element, and
 * resolving a volume id from its label via the title detail page (replacing a
 * brute-force `/volume/{1..N}` scan that broke once ids exceeded a hardcoded
 * cap under parallel load).
 */
import { Page, Locator, expect } from "@playwright/test";

/**
 * Select the first non-placeholder option of a `<select>` by its value rather
 * than by positional index (#22). Order-independent of any leading empty
 * "choose…" option, so it doesn't break if the option list is reordered.
 */
export async function selectFirstRealOption(select: Locator): Promise<void> {
  const value = await select
    .locator('option[value]:not([value=""])')
    .first()
    .getAttribute("value");
  expect(value, "select must expose at least one non-placeholder option").toBeTruthy();
  await select.selectOption(value!);
}

/**
 * Read the title id encoded in the scan skeleton element's id
 * (`id="feedback-entry-{titleId}"`). This is the designed contract from
 * `src/routes/catalog.rs::skeleton_feedback_html` — the only deterministic
 * title-id source the browser has after an ISBN scan. Centralized here so the
 * former inline copies in metadata-editing.spec.ts stay in sync.
 */
export async function titleIdFromSkeleton(page: Page): Promise<string> {
  await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
    timeout: 10000,
  });
  const feedbackEl = page.locator("[id^='feedback-entry-']").first();
  const feedbackId = await feedbackEl.getAttribute("id");
  const titleId = feedbackId?.replace("feedback-entry-", "");
  expect(
    titleId,
    "scan skeleton element must encode the title id in its element id",
  ).toBeTruthy();
  return titleId!;
}

/**
 * Resolve a volume's numeric id from its label by reading the volume link on
 * the title detail page — a deterministic replacement for the former
 * brute-force `/volume/{1..N}` fetch loop, which broke once volume ids
 * exceeded the hardcoded cap under parallel execution.
 */
export async function volumeIdByLabel(
  page: Page,
  titleId: string,
  label: string,
): Promise<string> {
  await page.goto(`/title/${titleId}`);
  const link = page
    .locator('a[href^="/volume/"]')
    .filter({ hasText: new RegExp(`^\\s*${label}\\s*$`) });
  const href = await link.first().getAttribute("href");
  expect(
    href,
    `title ${titleId} detail page must list a volume link for ${label}`,
  ).toBeTruthy();
  const id = href!.replace(/^\/volume\//, "").replace(/\/.*$/, "");
  expect(id).toMatch(/^\d+$/);
  return id;
}

/**
 * Story 17-4 / issue #676 — the id of the title named `name`, found through
 * the home search fragment, or null. The search ranks loosely: only a link
 * placed before this exact name counts, so give each title a unique name.
 */
export async function titleIdByName(page: Page, name: string): Promise<string | null> {
  const response = await page.request.get(`/?q=${encodeURIComponent(name)}`, {
    headers: { "HX-Request": "true" },
  });
  const html = await response.text();
  const at = html.indexOf(name);
  if (at < 0) return null;
  const links = [...html.slice(0, at).matchAll(/href="\/title\/(\d+)"/g)];
  return links.at(-1)?.[1] ?? null;
}

/**
 * Story 17-4 / issue #676 — a random 12-digit UPC-A whose check digit is
 * wrong (refused by every form, #384). Starts with 5: never an ISBN or
 * ISSN prefix.
 */
export function badUpcA(): string {
  let body = "5";
  while (body.length < 11) body += Math.floor(Math.random() * 10);
  const sum = [...body].reduce((acc, c, i) => acc + Number(c) * (i % 2 === 0 ? 3 : 1), 0);
  const valid = (10 - (sum % 10)) % 10;
  return body + String((valid + 1) % 10);
}
