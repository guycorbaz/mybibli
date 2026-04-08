import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./specs",
  // Per-spec unique ISBNs via helpers/isbn.ts eliminate ISBN collisions.
  // fullyParallel remains false because DEV_SESSION_COOKIE shares server-side
  // session state across workers and borrower names collide between specs.
  // Restoring fullyParallel: true requires per-test loginAs() + unique borrower names.
  fullyParallel: false,
  workers: 1,
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
