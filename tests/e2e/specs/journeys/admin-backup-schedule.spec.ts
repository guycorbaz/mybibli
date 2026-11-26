import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { backUpNow } from "../../helpers/backups";

/**
 * Story 19-7 (#657, FR179, FR180, UX-DR45) — automatic backups.
 *
 * - The admin sets "Every 6 hours" in Admin → System, reads the saved line,
 *   and the Health tab shows a "Next check" line. The setting is
 *   instance-wide: the spec puts it back as it found it in an `afterAll`
 *   on a fresh context, so a failed assertion or a timeout does not leave
 *   it changed. Only the backup scheduler
 *   reads it, so the spec stays in the parallel project.
 * - A value outside the list (an option changed in the page), and a
 *   retention outside 1..=365, are refused next to their field.
 * - After a "Back up now", the Health tab's last backup is stamped no
 *   earlier than the spec's own file: other specs write backups into the
 *   same folder (Rule #23), so it is never asserted to be exactly it.
 */

const SAVED =
  /Automatic backup saved: Every 6 hours\.|Sauvegarde automatique enregistrée : Toutes les 6 heures\.|Automatische Sicherung gespeichert: Alle 6 Stunden\.|Backup automatico salvato: Ogni 6 ore\./;
const NEXT_CHECK = /Next check|Prochaine vérification|Nächste Prüfung|Prossimo controllo/;
const REFUSED =
  /Choose one of the listed intervals\.|Choisissez l.un des intervalles proposés\.|Wählen Sie eines der angebotenen Intervalle\.|Scegli uno degli intervalli proposti\./;
const RETENTION_REFUSED =
  /Enter a whole number from 1 to 365\.|Indiquez un nombre entier entre 1 et 365\.|Geben Sie eine ganze Zahl von 1 bis 365 ein\.|Inserisci un numero intero da 1 a 365\./;
const STAMP = /mybibli-[0-9a-f]{16}-(\d{8}T\d{6}Z)\.tar\.gz/;

/** Save both Backups settings from Admin → System. */
async function saveSettings(page: Page, hours: string, days: string): Promise<void> {
  await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
  await page.locator("#backup-interval").selectOption(hours);
  await page.locator("#backup-retention").fill(days);
  const saved = page.waitForResponse(
    (r) => new URL(r.url()).pathname === "/admin/system/backups" && r.request().method() === "POST",
  );
  await page.locator("#admin-system-backups-submit").click();
  expect((await saved).status()).toBe(200);
}

/** The values found before the first test changed them. */
let original: { hours: string; days: string } | undefined;

test.describe("Story 19-7 — automatic backups", () => {
  // The tests read and write the same instance-wide settings.
  test.describe.configure({ mode: "serial" });

  // The restore runs in a fresh context: it holds even when a test timed
  // out and its page is closed.
  test.afterAll(async ({ browser }) => {
    if (original === undefined) return;
    const context = await browser.newContext({
      baseURL: process.env.BASE_URL || "http://localhost:8080",
    });
    try {
      const page = await context.newPage();
      await loginAs(page, "admin");
      await saveSettings(page, original.hours, original.days);
    } finally {
      await context.close();
    }
  });

  test("the admin sets the schedule and the Health tab shows the next check", async ({ page }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
    const select = page.locator("#backup-interval");
    await expect(select).toBeVisible();
    original = {
      hours: await select.inputValue(),
      days: await page.locator("#backup-retention").inputValue(),
    };

    await select.selectOption("6");
    await page.locator("#backup-retention").fill("21");
    await page.locator("#admin-system-backups-submit").click();
    await expect(page.locator("#feedback-list")).toContainText(SAVED);
    // The form is not swapped: the button keeps the focus (Rule #21).
    await expect(page.locator("#admin-system-backups-submit")).toBeFocused();
    await expect(select).toHaveValue("6");
    await expect(page.locator("#backup-retention-help")).toContainText("21");

    await page.goto("/admin?tab=health", { waitUntil: "domcontentloaded" });
    await expect(page.locator("#health-backup-next")).toContainText(NEXT_CHECK);
  });

  test("a value outside the list is refused next to the field", async ({ page }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
    const select = page.locator("#backup-interval");
    await expect(select).toBeVisible();
    const before = await select.inputValue();
    // A tampered form: the option "2" now posts "5".
    await page
      .locator('#backup-interval option[value="2"]')
      .evaluate((o) => ((o as HTMLOptionElement).value = "5"));
    await select.selectOption("5");
    await page.locator("#admin-system-backups-submit").click();
    await expect(page.locator("#backup-interval-error")).toContainText(REFUSED);

    // Nothing was stored.
    await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
    await expect(page.locator("#backup-interval")).toHaveValue(before);
  });

  test("a retention outside 1 to 365 is refused next to the field", async ({ page }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
    const days = page.locator("#backup-retention");
    await expect(days).toBeVisible();
    const before = await days.inputValue();
    // The browser's own check (min / max) would stop the form first: the
    // server's refusal is what this test is about.
    await days.evaluate((el) => {
      el.removeAttribute("min");
      el.removeAttribute("max");
    });
    await days.fill("366");
    await page.locator("#admin-system-backups-submit").click();
    await expect(page.locator("#backup-retention-error")).toContainText(RETENTION_REFUSED);
    await expect(page.locator("#backup-interval-error")).toBeEmpty();

    await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
    await expect(page.locator("#backup-retention")).toHaveValue(before);
  });

  test("after a backup, the Health tab shows a last backup no older than it", async ({ page }) => {
    test.setTimeout(200000);
    await loginAs(page, "admin");
    await page.goto("/admin?tab=backups", { waitUntil: "domcontentloaded" });
    const answer = await backUpNow(page);
    const mine = answer.match(STAMP);
    expect(mine, `a file name in: ${answer}`).not.toBeNull();

    await page.goto("/admin?tab=health", { waitUntil: "domcontentloaded" });
    const last = page.locator("#health-backup-last");
    await expect(last).toContainText(STAMP);
    const shown = ((await last.textContent()) ?? "").match(STAMP);
    expect(shown).not.toBeNull();
    // Same shape, UTC: the strings compare as times.
    expect(shown![1]! >= mine![1]!, `${shown![1]} is not earlier than ${mine![1]}`).toBe(true);
  });
});
