/**
 * Story 15-1 E2E (#602) — the instance time zone.
 *
 * Real journey from a blank browser: loginAs(admin) → catalogue a title
 * and a volume → create a borrower → seed a loan at 23:30 UTC on
 * 15 Jan 2026 (TEST_MODE seam) → the borrower page dates it 16 Jan (the
 * default zone, Europe/Zurich, is UTC+1 in winter) → Admin → System:
 * an unknown zone is refused, "UTC" is saved → the same page now dates
 * the loan 15 Jan, without restart → the zone is put back.
 *
 * The zone is a global setting: the describe is serial and restores
 * Europe/Zurich in `finally`. Switching to UTC cannot break a parallel
 * spec: the others cut their web days in UTC or compare instants.
 *
 * Spec ID "TZ" for ISBNs; V-codes V5600–V5699.
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { createBorrower, scanTitleAndVolume, seedOverdueLoan } from "../../helpers/loans";

async function saveTimeZone(page: Page, zone: string) {
  await page.goto("/admin?tab=system");
  const form = page.locator("#admin-system-time-zone-form");
  await form.locator("#time-zone").fill(zone);
  await form
    .getByRole("button", {
      name: /Save time zone|Enregistrer le fuseau horaire|Zeitzone speichern|Salva il fuso orario/i,
    })
    .click();
}

test.describe("Story 15-1 — instance time zone", () => {
  test.describe.configure({ mode: "serial" });

  test("a loan's day follows the zone set in Admin → System", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const seq = Math.floor(Math.random() * 100);
    const isbn = specIsbn("TZ", seq);
    const vcode = `V56${String(seq).padStart(2, "0")}`;
    const borrowerName = `TZ-Borrower ${crypto.randomUUID().slice(0, 8)}`;
    await scanTitleAndVolume(page, isbn, vcode);
    await createBorrower(page, borrowerName);
    await seedOverdueLoan(baseURL!, {
      volumeLabel: vcode,
      borrowerName,
      loanedAt: "2026-01-15 23:30:00",
    });

    const openBorrower = async () => {
      await page.goto("/borrowers");
      await page
        .locator('tbody a[href^="/borrower/"]')
        .filter({ hasText: borrowerName })
        .click();
      await expect(page).toHaveURL(/\/borrower\/\d+/);
    };
    const loanRow = () => page.locator("tbody tr").filter({ hasText: vcode });

    try {
      // Default zone (Europe/Zurich): 23:30 UTC is 00:30 the next day.
      await openBorrower();
      await expect(loanRow()).toContainText("2026-01-16");
      await expect(loanRow()).not.toContainText("2026-01-15");

      // An unknown zone is refused with a field error; nothing changes.
      await saveTimeZone(page, "Mars/Olympus_Mons");
      await expect(page.locator("#feedback-list")).toContainText(
        /Unknown time zone|Fuseau horaire inconnu|Unbekannte Zeitzone|Fuso orario sconosciuto/i,
      );
      await openBorrower();
      await expect(loanRow()).toContainText("2026-01-16");

      // UTC: the same loan reads 15 Jan, at once.
      await saveTimeZone(page, "UTC");
      await expect(page.locator("#feedback-list")).toContainText(
        /Time zone saved|Fuseau horaire enregistré|Zeitzone gespeichert|Fuso orario salvato/i,
      );
      await expect(page.locator("#time-zone")).toHaveValue("UTC");
      await openBorrower();
      await expect(loanRow()).toContainText("2026-01-15");
    } finally {
      await saveTimeZone(page, "Europe/Zurich");
      await expect(page.locator("#time-zone")).toHaveValue("Europe/Zurich");
    }
  });
});
