import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";

/**
 * Story 19-5 (#708) — Admin → Health "Production watching": in-memory
 * counts of slow statements, failed connection waits, CSRF refusals and
 * API-key refusals, with the most frequent client addresses.
 *
 * Spec id "HW" (no ISBN, no V-code). The counters are instance-wide and
 * other workers add refusals at the same time (Rule #23): every assertion
 * is "increased by at least N", never an exact value.
 */

async function openHealth(page: Page) {
  await page.goto("/admin?tab=health");
  await expect(page.locator("#health-watch")).toBeVisible();
}

async function readCount(page: Page, id: string): Promise<number> {
  const text = (await page.locator(`#${id} [data-watch-count]`).innerText()).trim();
  const n = Number.parseInt(text, 10);
  expect(Number.isNaN(n), `${id} should hold a number, got "${text}"`).toBe(false);
  return n;
}

test.describe("Story 19-5 — production watching on the Health tab", () => {
  test("a refused CSRF token is counted, and above 20 it is marked as unusual", async ({
    page,
  }) => {
    await loginAs(page);
    await openHealth(page);
    await expect(
      page.getByRole("heading", {
        name: /Production watching|Surveillance de la production|Überwachung des Betriebs|Sorveglianza della produzione/i,
      }),
    ).toBeVisible();
    await expect(page.locator("#watch-since-start-heading")).toContainText(
      /\d{4}-\d{2}-\d{2} \d{2}:\d{2}/,
    );
    const before = await readCount(page, "watch-csrf-refusals-total");

    // 21 refusals: more than the threshold (20) on their own, whatever
    // the other workers do.
    for (let i = 0; i < 21; i++) {
      const resp = await page.request.post("/logout", {
        headers: { "X-CSRF-Token": "tampered-token" },
        form: { _csrf_token: "tampered-token" },
        maxRedirects: 0,
      });
      expect(resp.status()).toBe(403);
    }

    await openHealth(page);
    const after = await readCount(page, "watch-csrf-refusals-total");
    expect(after).toBeGreaterThanOrEqual(before + 21);
    expect(await readCount(page, "watch-csrf-refusals-24h")).toBeGreaterThanOrEqual(21);
    await expect(page.locator("#watch-csrf-refusals-24h [data-watch-marker]")).toContainText(
      /above the usual level|au-dessus du niveau habituel|über dem üblichen Niveau|sopra il livello abituale/i,
    );
    // The refusals came from this browser's address: at least one listed.
    expect(
      await page.locator("#watch-csrf-refusals-addresses li").count(),
    ).toBeGreaterThanOrEqual(1);
    await expect(page.locator("#health-watch")).toContainText(
      /a restart resets them|un redémarrage les remet à zéro|ein Neustart setzt sie zurück|un riavvio li azzera/i,
    );
    await checkAccessibility(page);
  });

  test("an API request with a wrong key is counted", async ({ page }) => {
    await loginAs(page);
    await openHealth(page);
    const before = await readCount(page, "watch-api-key-refusals-total");

    const resp = await page.request.get("/api/v1/series", {
      headers: { Authorization: "Bearer mybibli_not-a-real-key" },
    });
    expect(resp.status()).toBe(401);

    await openHealth(page);
    const after = await readCount(page, "watch-api-key-refusals-total");
    expect(after).toBeGreaterThanOrEqual(before + 1);
    expect(
      await page.locator("#watch-api-key-refusals-addresses li").count(),
    ).toBeGreaterThanOrEqual(1);
  });

  test("a librarian never sees the panel", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/admin?tab=health");
    await expect(page.locator("#health-watch")).toHaveCount(0);
  });
});
