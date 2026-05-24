import { request, APIRequestContext } from "@playwright/test";

export interface EntityCounts {
  titles: number;
  volumes: number;
  contributors: number;
  borrowers: number;
  active_loans: number;
}

/**
 * CR #43 — Snapshot the key entity counts surfaced by `/admin/health`.
 *
 * Used to PROVE that a 4xx/3xx rejected mutation (CSRF rejection,
 * unauthorized redirect, role-gate refusal) actually left the DB
 * untouched. Status-code-only assertions cannot catch a future
 * regression where the middleware rejects with the right status while
 * the mutation also fires — `expect(after).toEqual(before)` does.
 *
 * The helper spins up its own `APIRequestContext` authenticated as
 * Admin so the caller test can keep its own cookie state (typically
 * anonymous) intact. The auth + fetch + dispose round-trip costs ~150 ms.
 *
 * Counts are scraped from the admin/health HTML fragment in the
 * project-locked render order (titles, volumes, contributors,
 * borrowers, active_loans) — see `templates/fragments/admin_health_panel.html`.
 * APP_LANGUAGE in `docker-compose.test.yml` is pinned to `en`, so the
 * regex does not need to be locale-aware.
 *
 * @param baseURL — passed via `testInfo.project.use.baseURL`
 * @throws on auth failure or unexpected admin/health markup shape
 */
export async function captureEntityCounts(baseURL: string): Promise<EntityCounts> {
  const ctx: APIRequestContext = await request.newContext({ baseURL });
  try {
    // Login as admin — mirror loginAs() helper. CSRF token sits on the
    // /login form as a hidden input; we scrape it before posting.
    const loginHtml = await (await ctx.get("/login")).text();
    const csrfMatch = /name="_csrf_token"\s+value="([^"]+)"/.exec(loginHtml);
    if (!csrfMatch) {
      throw new Error("captureEntityCounts: _csrf_token not found on /login");
    }
    const csrf = csrfMatch[1];

    const username = process.env.TEST_ADMIN_USERNAME ?? "admin";
    const password = process.env.TEST_ADMIN_PASSWORD ?? "admin";
    const loginResp = await ctx.post("/login", {
      form: { _csrf_token: csrf, username, password },
      maxRedirects: 0,
      failOnStatusCode: false,
    });
    if (loginResp.status() !== 303) {
      throw new Error(
        `captureEntityCounts: admin login failed (status ${loginResp.status()})`,
      );
    }

    // Fetch admin/health as an HTMX fragment to skip the surrounding
    // page chrome. Falls back to the full page if HX-Request handling
    // ever changes — the regex below works against either shape.
    const panelResp = await ctx.get("/admin/health", {
      headers: { "HX-Request": "true" },
    });
    const html = await panelResp.text();

    // Parse the 5 count cells. Stable hook is `class="…font-mono…">123</td>`
    // — same `font-mono` class the project uses everywhere for numeric
    // cells; locked by `templates/fragments/admin_health_panel.html`.
    const re = /font-mono[^>]*>\s*([\d,]+)\s*<\/td>/g;
    const matches: number[] = [];
    let m: RegExpExecArray | null;
    while ((m = re.exec(html)) !== null) {
      matches.push(parseInt(m[1].replace(/,/g, ""), 10));
    }
    if (matches.length < 5) {
      throw new Error(
        `captureEntityCounts: expected ≥5 numeric cells in admin/health, got ${matches.length}`,
      );
    }
    return {
      titles: matches[0],
      volumes: matches[1],
      contributors: matches[2],
      borrowers: matches[3],
      active_loans: matches[4],
    };
  } finally {
    await ctx.dispose();
  }
}
