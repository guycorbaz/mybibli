import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./specs",
  // Per-spec unique ISBNs (helpers/isbn.ts) + per-test loginAs() sessions
  // ensure full data isolation between parallel workers.
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  // Story 19-2 (D1) — a test that passes only on a retry is flaky, and a
  // flaky test fails CI: it is seen, diagnosed and fixed, never waved
  // through. Retries stay so the report shows which attempt failed.
  failOnFlakyTests: !!process.env.CI,
  // CI also prints each result in the job log (the failing assertion is
  // there without downloading the HTML report).
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "html",
  use: {
    baseURL: process.env.BASE_URL || "http://localhost:8080",
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { browserName: "chromium" },
    },
  ],
});
