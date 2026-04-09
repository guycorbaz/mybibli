import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./specs",
  // Per-spec unique ISBNs (helpers/isbn.ts) + per-test loginAs() sessions
  // ensure full data isolation between parallel workers.
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: "html",
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
