import { defineConfig } from "@playwright/test";

/** Specs that change instance-wide settings (story 19-2): run last, alone. */
const SETTINGS_SPEC = /journeys\/admin-system-settings\.spec\.ts/;
const SESSION_TIMEOUT_SPEC = /journeys\/session-inactivity-timeout\.spec\.ts/;
/** Story 19-8 — replaces the whole database and logs everyone out: last of all. */
const RESTORE_SPEC = /journeys\/admin-backup-restore\.spec\.ts/;
/**
 * Story 19-9 — the cover repair acts on the whole instance (every orphan
 * file deleted, every missing cover address cleared): after every other
 * test, alone, and before the restore.
 */
const COVER_CHECK_SPEC = /journeys\/admin-cover-check\.spec\.ts/;
const INSTANCE_WIDE_SPECS = [SETTINGS_SPEC, SESSION_TIMEOUT_SPEC, COVER_CHECK_SPEC, RESTORE_SPEC];

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
    // Cost accepted: a failure in `chromium` skips them (the job is red
    // anyway). Run one alone with `--no-deps`, or the whole suite runs first.
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
    // Story 19-9 — the cover repair deletes every orphan cover file and
    // clears every missing cover address of the instance, so it would
    // disturb the rows of parallel specs (Rule #23): after every other
    // test, alone. Before the restore, which stays last of all: the restore
    // logs everyone out and replaces the database and the covers folder,
    // and it is the project story 19-10's epic smoke joins.
    {
      name: "instance-cover-check",
      use: { browserName: "chromium" },
      testMatch: COVER_CHECK_SPEC,
      dependencies: ["instance-session-timeout"],
    },
    // Story 19-8 (AC9) — a restore replaces the whole database and logs
    // everyone out: after every other test, alone. It restores a backup
    // it wrote itself moments before, so the stack stays usable afterwards
    // (the library it found, the seeded users). Story 19-10's epic smoke
    // joins this project.
    {
      name: "instance-restore",
      use: { browserName: "chromium" },
      testMatch: RESTORE_SPEC,
      dependencies: ["instance-cover-check"],
    },
  ],
});
