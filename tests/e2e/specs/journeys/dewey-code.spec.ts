import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { createLocation } from "../../helpers/locations";

test.describe("Dewey Code Management (Story 5-8)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("librarian can edit and persist Dewey code", async ({ page }) => {
    const ISBN = specIsbn("DC", 1);

    // Scan ISBN to create title
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(ISBN);
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    // Navigate to title detail via search
    await page.goto(`/?q=${ISBN}`);
    await page.locator('a[href^="/title/"]').first().click();
    await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });

    // Click Edit metadata
    await page
      .getByRole("button", {
        name: /Edit metadata|Modifier les métadonnées/i,
      })
      .click();
    await expect(page.locator("#edit-dewey")).toBeVisible({ timeout: 5000 });

    // Fill Dewey code
    await page.locator("#edit-dewey").fill("843.914");

    // Fill empty numeric fields to avoid 422 (empty string → invalid i32)
    const pageCount = page.locator("#edit-page-count");
    if (await pageCount.isVisible({ timeout: 500 }).catch(() => false)) {
      const val = await pageCount.inputValue();
      if (!val) await pageCount.fill("0");
    }

    // Click Save via type=submit (canonical pattern from metadata-editing.spec.ts)
    await page.locator('main button[type="submit"]').last().click();

    // Verify Dewey appears in metadata display after HTMX swap
    await expect(page.locator("#title-metadata")).toContainText("843.914", {
      timeout: 10000,
    });

    // Verify i18n label is used (not hardcoded "Dewey:")
    await expect(page.locator("#title-metadata")).toContainText(
      /Dewey code|Code Dewey/i,
    );
  });

  test("location view sorts by Dewey with NULL last", async ({ page }) => {
    const ISBN_DEWEY_200 = specIsbn("DC", 2);
    const ISBN_NO_DEWEY = specIsbn("DC", 3);
    const ISBN_DEWEY_900 = specIsbn("DC", 4);
    const lcode = await createLocation(page, "DC-DeweySort", "L5801");

    const scanField = page.locator("#scan-field");

    // Helper: scan ISBN, create a volume, shelve it at `lcode`
    async function scanAndShelve(isbn: string, vcode: string) {
      await page.goto("/catalog");
      await scanField.fill(isbn);
      await scanField.press("Enter");
      await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
        timeout: 10000,
      });
      await scanField.fill(vcode);
      await scanField.press("Enter");
      await expect(
        page.locator(".feedback-entry").first(),
      ).toContainText(new RegExp(vcode, "i"), { timeout: 10000 });
      await scanField.fill(lcode);
      await scanField.press("Enter");
      await expect(
        page.locator(".feedback-entry").first(),
      ).toContainText(new RegExp(lcode + "|shelved|rangé", "i"), {
        timeout: 5000,
      });
    }

    // Helper: open title detail from search and set its Dewey code
    async function setDeweyViaEdit(isbn: string, dewey: string) {
      await page.goto(`/?q=${isbn}`);
      await page.locator('a[href^="/title/"]').first().click();
      await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });
      await page
        .getByRole("button", {
          name: /Edit metadata|Modifier les métadonnées/i,
        })
        .click();
      await expect(page.locator("#edit-dewey")).toBeVisible({ timeout: 5000 });
      await page.locator("#edit-dewey").fill(dewey);
      const pc = page.locator("#edit-page-count");
      if (await pc.isVisible({ timeout: 500 }).catch(() => false)) {
        const val = await pc.inputValue();
        if (!val) await pc.fill("0");
      }
      await page.locator("#edit-title-submit").click();
      await expect(page.locator("#title-metadata")).toContainText(dewey, {
        timeout: 10000,
      });
    }

    // Seed 3 volumes at same location:
    //   - title with Dewey "200"   (V5801)
    //   - title with no Dewey      (V5802)
    //   - title with Dewey "900"   (V5803)
    // Two non-NULL values make the DESC-with-NULL-last assertion non-trivial:
    // a buggy impl that omits `IS NULL` in DESC would return ["900","200",NULL]
    // from MariaDB default (which happens to be right) OR would return
    // [NULL,"900","200"] if default is NULL-first — detection requires >= 2 non-NULL.
    await scanAndShelve(ISBN_DEWEY_200, "V5801");
    await setDeweyViaEdit(ISBN_DEWEY_200, "200");

    await scanAndShelve(ISBN_NO_DEWEY, "V5802");

    await scanAndShelve(ISBN_DEWEY_900, "V5803");
    await setDeweyViaEdit(ISBN_DEWEY_900, "900");

    // --- Find location ID via edit link on /locations ---
    await page.goto("/locations");
    const editLink = page.locator('a[aria-label*="DC-DeweySort"]').first();
    await expect(editLink).toBeVisible({ timeout: 5000 });
    const href = await editLink.getAttribute("href");
    const locId = href?.match(/\/locations\/(\d+)/)?.[1];
    expect(locId).toBeTruthy();

    const rows = page.locator("table tbody tr");
    // Assertion strategy — resilient to column-order changes:
    // - Non-NULL Dewey rows render a <code class="font-mono">{value}</code> cell.
    // - NULL Dewey rows render an em-dash and NO <code> element.
    // So `row.locator("code")` is the semantic locator for "does this row have a Dewey?"

    // --- ASC: expect order [200, 900, NULL] ---
    await page.goto(`/location/${locId}?sort=dewey_code&dir=asc`);
    await expect(rows).toHaveCount(3, { timeout: 5000 });

    await expect(rows.nth(0).locator("code")).toContainText("200");
    await expect(rows.nth(1).locator("code")).toContainText("900");
    // Row 2 is the NULL-Dewey row: no <code> element on the row
    await expect(rows.nth(2).locator("code")).toHaveCount(0);

    // --- DESC: expect order [900, 200, NULL] — non-trivial: NULL must STILL be last ---
    await page.goto(`/location/${locId}?sort=dewey_code&dir=desc`);
    await expect(rows).toHaveCount(3, { timeout: 5000 });

    await expect(rows.nth(0).locator("code")).toContainText("900");
    await expect(rows.nth(1).locator("code")).toContainText("200");
    await expect(rows.nth(2).locator("code")).toHaveCount(0);

    // Verify column header is present (may include sort arrow ▲/▼)
    await expect(
      page.locator("th").filter({ hasText: /Dewey/ }),
    ).toBeVisible();
  });
});
