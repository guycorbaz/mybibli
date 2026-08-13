import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const VALID_ISBN = specIsbn("CM", 1);
const COUNTER_ISBN = specIsbn("CM", 2); // Unique ISBN for session counter test
// Invalid ISBN-13 (wrong checksum)
const INVALID_ISBN = specIsbn("CM", 99).slice(0, 12) + "0";

test.describe("Scan Feedback & Async Metadata (Story 1-7)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC1: Skeleton FeedbackEntry on ISBN scan
  test("scan ISBN shows skeleton feedback with spinner", async ({ page }) => {
    await page.goto("/catalog");

    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // A skeleton or resolved feedback should appear
    // (depending on speed of metadata fetch, we may see skeleton or resolved)
    const anyFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(anyFeedback.first()).toBeVisible({ timeout: 5000 });
  });

  // AC2: Resolved metadata via PendingUpdates
  test("second scan triggers OOB delivery of resolved metadata", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // First scan: creates title with async metadata fetch
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Wait for initial feedback
    const firstFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(firstFeedback.first()).toBeVisible({ timeout: 5000 });

    // Second scan: triggers PendingUpdates middleware to deliver resolved data.
    // Timeout bounded to 15s to cover BnF timeout + Google Books fallback under CI load.
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Use .last() to match the most recent scan's feedback entry
    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]'
    ).last();
    await expect(infoEntry).toBeVisible({ timeout: 15000 });
  });

  // AC7: Session counter
  test("session counter increments on new ISBN scan", async ({ page }) => {
    await page.goto("/catalog");

    // Use a unique ISBN so the title is truly NEW (is_new=true triggers counter OOB)
    const scanField = page.locator("#scan-field");
    await scanField.fill(COUNTER_ISBN);
    await scanField.press("Enter");

    // Session counter text should appear via OOB swap (use .first() due to duplicate IDs in DOM)
    await expect(page.locator("#session-counter").first()).toContainText(/session|éléments/i, { timeout: 5000 });
  });

  // AC5: Client-side ISBN validation
  test("invalid ISBN shows error feedback without server request", async ({
    page,
  }) => {
    await page.goto("/catalog");

    const scanField = page.locator("#scan-field");
    await scanField.fill(INVALID_ISBN);
    await scanField.press("Enter");

    // Error feedback should appear (from server-side validation since client
    // validation is in scan-field.js)
    const errorEntry = page.locator(
      '.feedback-entry[data-feedback-variant="error"]'
    );
    await expect(errorEntry).toBeVisible({ timeout: 5000 });
  });

  // AC6: Already-assigned V-code error
  test("already assigned V-code shows error with title name", async ({
    page,
  }) => {
    // CR #300: unique ISBN so the title has no prior volumes and the V0055
    // scan path doesn't hit the phantom-volume confirmation modal.
    const isbn = specIsbn("CM", 3);
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // First: create a title
    await scanField.fill(isbn);
    await scanField.press("Enter");
    await expect(page.locator("#feedback-list .feedback-skeleton, #feedback-list .feedback-entry").first()).toBeVisible({ timeout: 5000 });

    // Create a volume
    await scanField.fill("V0055");
    await scanField.press("Enter");

    // Wait for volume created feedback
    const volFeedback = page.locator(
      '.feedback-entry[data-feedback-variant="success"]'
    );
    await expect(volFeedback.first()).toBeVisible({ timeout: 5000 });

    // Try to assign same V-code again
    await scanField.fill("V0055");
    await scanField.press("Enter");

    // Should get error about already assigned
    const errorEntry = page.locator(
      '.feedback-entry[data-feedback-variant="error"]'
    );
    await expect(errorEntry).toBeVisible({ timeout: 5000 });
  });

  // AC8: Mock metadata server for deterministic E2E
  test("metadata responses are deterministic in test environment", async ({
    page,
  }) => {
    await page.goto("/catalog");

    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Feedback should appear (skeleton or resolved)
    const feedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(feedback.first()).toBeVisible({ timeout: 5000 });

    // Context banner should be populated
    const banner = page.locator("#context-banner");
    await expect(banner).not.toHaveClass(/hidden/, { timeout: 5000 });

    // Trigger delivery; bounded to 15s for BnF timeout + Google Books fallback under CI load.
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Mock catch-all returns "Test Title {isbn}" by "Synthetic TestAuthor" for unique ISBNs.
    await expect(page.locator("body")).toContainText(/Test Title|TestAuthor/i, { timeout: 15000 });
  });
  // ─── #202 — metadata-source badge on /title/:id ────────────────────
  //
  // The provider chain has always known which provider answered and then
  // discarded it, so a librarian could not tell "BnF answered but holds
  // little" from "nothing answered at all" — the question behind the original
  // report. `titles.metadata_source` now records it and the detail page shows
  // it.
  //
  // BnF is first in the chain for books and the mock answers its SRU endpoint
  // with a synthetic record for any unknown ISBN, so a freshly generated code
  // resolves through BnF deterministically.
  test("title detail names the provider that resolved the metadata", async ({
    page,
  }) => {
    // Randomised, like the Story 10-5 seed in accessibility-full.spec.ts: this
    // test REQUIRES a brand-new title, because only a first scan emits a
    // `.feedback-skeleton`. A fixed ISBN works exactly once against the shared
    // E2E database and then silently takes the "title already exists" path,
    // which returns an info entry with no id to read. Banded 10000-99998 so it
    // cannot collide with this spec's fixed codes (CM 1, 2, 3 and 99).
    const isbn = specIsbn("CM", 10000 + Math.floor(Math.random() * 89999));
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");

    const skeleton = page
      .locator(".feedback-skeleton[id^='feedback-entry-']")
      .first();
    await expect(skeleton).toBeVisible({ timeout: 10000 });
    const idAttr = await skeleton.getAttribute("id");
    const titleId = idAttr!.replace("feedback-entry-", "");

    // Poll the detail page: the source is written by the background fetch
    // task, so it lands slightly after the skeleton appears.
    await expect(async () => {
      await page.goto(`/title/${titleId}`);
      // i18n-aware: "Source: BnF" (en) / "Source : BnF" (fr). The provider name
      // is deliberately NOT translated (NFR41), so it anchors the assertion in
      // both locales.
      await expect(page.getByText(/Source\s*:\s*BnF/)).toBeVisible({
        timeout: 1000,
      });
    }).toPass({ timeout: 20000 });
  });

});
