/**
 * #21 (DB hardening) — `/health` DB-readiness probe.
 *
 * The endpoint was promoted from a bare process-alive stub (`"ok"` always)
 * to a readiness check that runs `SELECT 1` against the pool and returns
 * `200 "ok"` only when the database is reachable. This spec covers the happy
 * path against the live MariaDB the E2E stack runs against — the unreachable
 * 503 branch is unit-tested in `src/routes/mod.rs` against a lazy pool.
 *
 * Spec ID "HC" — no catalog rows created, no login required (the probe is on
 * the setup-gate / CSP whitelist and is intentionally unauthenticated).
 */
import { test, expect } from "@playwright/test";

test.describe("#21 — /health DB-readiness probe", () => {
  test("returns 200 'ok' when the database is reachable", async ({ request }) => {
    const response = await request.get("/health");
    expect(response.status()).toBe(200);
    expect((await response.text()).trim()).toBe("ok");
  });
});
