/**
 * Story 18-4 (#599) — `GET /api/v1/series` lists every series, not page 1
 * of the `/series` list (25 rows).
 *
 * From a blank browser: an admin logs in, creates 27 series under a
 * per-run unique `zzz-api-` prefix (Rule #23: the spec asserts only on its
 * own series; whatever else the shared table holds, at least 26 sort
 * before the last one, which used to fall off the first page), creates a
 * read-only API key, reads `/api/v1/series` with it, and revokes the key.
 *
 * File name: it creates series, so it must sort after the specs that
 * assume none exists (`classification-fieldset`, `dewey-code`,
 * `empty-states` — #528, D-11-11-6); `api-series` ran first and broke them.
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";

const COUNT = 27;

async function createSeries(page: Page, name: string): Promise<void> {
  const csrf = await page.locator('meta[name="csrf-token"]').getAttribute("content");
  const resp = await page.request.post("/series", {
    form: { _csrf_token: csrf ?? "", name, series_type: "open" },
    maxRedirects: 0,
  });
  expect(resp.status(), `create series ${name}`).toBe(303);
}

test.describe("Story 18-4 — the series API returns every series (#599)", () => {
  test("a read key lists all the series a library has, past the 25th", async ({ page }) => {
    test.setTimeout(90000);
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const prefix = `zzz-api-${Date.now().toString(36)}`;
    const names = Array.from({ length: COUNT }, (_, i) => `${prefix}-${String(i + 1).padStart(2, "0")}`);
    await page.goto("/series");
    for (const name of names) await createSeries(page, name);

    const key = await createApiKey(page, "read", "api-series");
    try {
      const resp = await page.request.get("/api/v1/series", {
        headers: { Authorization: `Bearer ${key.plaintext}` },
      });
      expect(resp.status()).toBe(200);
      const body = (await resp.json()) as Array<{ name: string; series_type: string }>;
      const ours = body.map((s) => s.name).filter((n) => n.startsWith(`${prefix}-`));
      // All of them, in name order — the last one sorts after at least 26.
      expect(ours).toEqual(names);
      expect(body.find((s) => s.name === names[COUNT - 1])?.series_type).toBe("open");
    } finally {
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, key.label);
    }
  });
});
