import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { backUpNow } from "../../helpers/backups";

/**
 * Story 19-7 (#657, FR179, FR180, UX-DR45) — automatic backups.
 *
 * - The admin sets "Every 6 hours" in Admin → System, reads the saved line,
 *   and the Health tab shows a "Next check" line. The setting is
 *   instance-wide: the spec puts it back as it found it in a `finally`, so
 *   a failed assertion does not leave it changed. Only the backup scheduler
 *   reads it, so the spec stays in the parallel project.
 * - A value outside the list (an option changed in the page) is refused
 *   next to the field.
 * - After a "Back up now", the Health tab's last backup is stamped no
 *   earlier than the spec's own file: other specs write backups into the
 *   same folder (Rule #23), so it is never asserted to be exactly it.
 */

const SAVED =
  /Automatic backup saved: Every 6 hours\.|Sauvegarde automatique enregistrée : Toutes les 6 heures\.|Automatische Sicherung gespeichert: Alle 6 Stunden\.|Backup automatico salvato: Ogni 6 ore\./;
const NEXT_CHECK = /Next check|Prochaine vérification|Nächste Prüfung|Prossimo controllo/;
const REFUSED =
  /Choose one of the listed intervals\.|Choisissez l.un des intervalles proposés\.|Wählen Sie eines der angebotenen Intervalle\.|Scegli uno degli intervalli proposti\./;
const STAMP = /mybibli-[0-9a-f]{16}-(\d{8}T\d{6}Z)\.tar\.gz/;

async function saveInterval(page: Page, hours: string): Promise<void> {
  await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
  await page.locator("#backup-interval").selectOption(hours);
  const saved = page.waitForResponse(
    (r) => new URL(r.url()).pathname === "/admin/system/backups" && r.request().method() === "POST",
  );
  await page.locator("#admin-system-backups-submit").click();
  expect((await saved).status()).toBe(200);
}

test.describe("Story 19-7 — automatic backups", () => {
  // The first two tests read and write the same instance-wide setting.
  test.describe.configure({ mode: "serial" });

  test("the admin sets the schedule and the Health tab shows the next check", async ({ page }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system", { waitUntil: "domcontentloaded" });
    const select = page.locator("#backup-interval");
    await expect(select).toBeVisible();
    const original = await select.inputValue();
    try {
      await select.selectOption("6");
      await page.locator("#admin-system-backups-submit").click();
      await expect(page.locator("#feedback-list")).toContainText(SAVED);
      // The form is not swapped: the button keeps the focus (Rule #21).
      await expect(page.locator("#admin-system-backups-submit")).toBeFocused();
      await expect(select).toHaveValue("6");

      await page.goto("/admin?tab=health", { waitUntil: "domcontentloaded" });
      await expect(page.locator("#health-backup-next")).toContainText(NEXT_CHECK);
    } finally {
      await saveInterval(page, original);
    }
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

  test("after a backup, the Health tab shows a last backup no older than it", async ({ page }) => {
    test.setTimeout(120000);
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
