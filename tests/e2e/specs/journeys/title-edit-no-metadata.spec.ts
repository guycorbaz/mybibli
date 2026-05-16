import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

// Sentinel ISBN: the mock metadata server returns NO metadata from any
// provider for this code (see NO_METADATA_ISBNS in mock-metadata-server/server.py).
// Used to reproduce the user-reported scenario in issue #203: scan a code
// that the provider chain cannot resolve, then save edits manually.
const NO_METADATA_ISBN = "9780000000019";

test.describe("Title edit after no metadata (#203)", () => {
  // The scan response shape (skeleton vs. info) depends on whether the title
  // already exists. Retries would land on the info-variant path and the
  // skeleton id regex would not match — disable retries so the first attempt
  // is the source of truth.
  test.describe.configure({ retries: 0 });

  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // Direct API for both scan and save instead of the HTMX form. Submitting
  // through the form proved sensitive to DB state across retries and to the
  // intermediate HTMX retarget/swap pipeline. Mirrors the createLoan helper.
  test("scan no-metadata ISBN, edit title, save with picked genre", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const csrfToken = await page.evaluate(() =>
      document.querySelector('meta[name="csrf-token"]')?.getAttribute("content")
    );
    expect(csrfToken, "csrf token meta tag must be present").toBeTruthy();

    const scanResp = await page.request.post("/catalog/scan", {
      form: { code: NO_METADATA_ISBN, _csrf_token: csrfToken ?? "" },
      headers: { "HX-Request": "true", "X-CSRF-Token": csrfToken ?? "" },
    });
    expect(scanResp.ok(), `scan failed: ${scanResp.status()}`).toBe(true);
    const scanHtml = await scanResp.text();
    const idMatch = scanHtml.match(/id="feedback-entry-(\d+)"/);
    expect(
      idMatch,
      `no skeleton id in scan response. Response was:\n${scanHtml.slice(0, 500)}`
    ).toBeTruthy();
    const titleId = idMatch![1];

    // Read the list of active genres from the edit form so we pick a real
    // (non-default) id without hardcoding it.
    const editFormResp = await page.request.get(`/title/${titleId}/edit`);
    expect(editFormResp.ok()).toBe(true);
    const editFormHtml = await editFormResp.text();
    const versionMatch = editFormHtml.match(
      /name="version" value="(\d+)"/
    );
    expect(versionMatch).toBeTruthy();
    const version = versionMatch![1];

    const romanMatch = editFormHtml.match(/<option value="(\d+)"[^>]*>Roman</);
    expect(romanMatch, "Roman genre must exist in the edit form").toBeTruthy();
    const romanId = romanMatch![1];

    // Save: emulate the form's POST. Use a real genre + a real title so the
    // assertion below distinguishes success from "form unchanged".
    const saveResp = await page.request.post(`/title/${titleId}`, {
      form: {
        version,
        title: "Test no-metadata book",
        language: "fr",
        genre_id: romanId,
        _csrf_token: csrfToken ?? "",
      },
      headers: { "HX-Request": "true", "X-CSRF-Token": csrfToken ?? "" },
    });
    expect(saveResp.ok(), `save failed: ${saveResp.status()}`).toBe(true);
    const saveHtml = await saveResp.text();
    expect(saveHtml).toContain("Test no-metadata book");
    expect(saveHtml).toContain("Roman");

    // And the persisted state is reflected on a fresh navigation, ruling out
    // a transient response that wasn't actually written.
    await page.goto(`/title/${titleId}`);
    await expect(page.locator("#title-metadata")).toContainText(
      "Test no-metadata book"
    );
    await expect(page.locator("#title-metadata")).toContainText("Roman");
  });

  // Defensive-fallback contract: even if the form submits with no genre_id
  // (sentinel #[serde(default)] = 0 — empty <select>, corrupt row, or
  // missing field), the save resolves to the "Non classé" default rather
  // than failing with an FK violation. This is what #203's fix added.
  test("save with missing genre_id falls back to Non classé", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const csrfToken = await page.evaluate(() =>
      document.querySelector('meta[name="csrf-token"]')?.getAttribute("content")
    );
    expect(csrfToken).toBeTruthy();

    // Use a different ISBN so this case has its own row, independent of the
    // first test's title.
    const ISBN = "9780000000026"; // Valid checksum (computed by hand)

    const scanResp = await page.request.post("/catalog/scan", {
      form: { code: ISBN, _csrf_token: csrfToken ?? "" },
      headers: { "HX-Request": "true", "X-CSRF-Token": csrfToken ?? "" },
    });
    expect(scanResp.ok()).toBe(true);
    const scanHtml = await scanResp.text();
    const idMatch = scanHtml.match(/id="feedback-entry-(\d+)"/);
    expect(idMatch).toBeTruthy();
    const titleId = idMatch![1];

    const editFormResp = await page.request.get(`/title/${titleId}/edit`);
    const versionMatch = (await editFormResp.text()).match(
      /name="version" value="(\d+)"/
    );
    const version = versionMatch![1];

    // genre_id intentionally omitted from the form — `#[serde(default)] u64`
    // makes it 0 server-side, exercising the fallback.
    const saveResp = await page.request.post(`/title/${titleId}`, {
      form: {
        version,
        title: "Missing-genre fallback",
        language: "fr",
        _csrf_token: csrfToken ?? "",
      },
      headers: { "HX-Request": "true", "X-CSRF-Token": csrfToken ?? "" },
    });
    expect(
      saveResp.ok(),
      `save with missing genre_id failed: ${saveResp.status()}`
    ).toBe(true);
    const saveHtml = await saveResp.text();
    expect(saveHtml).toContain("Non classé");
  });
});
