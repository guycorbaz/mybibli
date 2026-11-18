import { defineConfig } from "@playwright/test";

/** Specs that change instance-wide settings (story 19-2): run last, alone. */
const SETTINGS_SPEC = /journeys\/admin-system-settings\.spec\.ts/;
const SESSION_TIMEOUT_SPEC = /journeys\/session-inactivity-timeout\.spec\.ts/;
const INSTANCE_WIDE_SPECS = [SETTINGS_SPEC, SESSION_TIMEOUT_SPEC];

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
      testIgnore: INSTANCE_WIDE_SPECS,
    },
    // Story 19-2 — specs that change an instance-wide setting while they run
    // (System settings saved; the session timeout dropped to 9 s) run after
    // every other test, one file at a time (a project each, chained):
    // `mode: "serial"` only serialises their own file, and the other
    // worker's sessions expired or showed the timeout toast meanwhile.
    {
      name: "instance-settings",
      use: { browserName: "chromium" },
      testMatch: SETTINGS_SPEC,
      dependencies: ["chromium"],
    },
    {
      name: "instance-session-timeout",
      use: { browserName: "chromium" },
      testMatch: SESSION_TIMEOUT_SPEC,
      dependencies: ["instance-settings"],
    },
  ],
});
