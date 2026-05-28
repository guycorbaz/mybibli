import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

// CR #355 — "Titles without cover" home filter chip. Discovery path to the
// manual-upload safety net for the FR/CH/DE tech-publisher titles no free
// provider indexes. Librarian+ only. Mirrors the no_volumes / uncategorized
// chip pattern. The SQL filtering correctness is locked by the integration
// tests in tests/search_filter_browse.rs; this spec covers the user-facing
// affordance: chip visible, navigates, active-state clears, hidden for anon.

const LABEL = /Titles without cover|Titres sans couverture|Titel ohne Cover|Titoli senza copertina/i;
const REMOVE = /Remove filter|Retirer le filtre|Filter entfernen|Rimuovi filtro/i;

test.describe("Home — no-cover filter chip (#355)", () => {
  test("librarian: chip visible, navigates to ?filter=no_cover, active state clears", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    // Inactive chip is a link labelled with the localized text.
    const chip = page.getByRole("link", { name: LABEL });
    await expect(chip).toBeVisible();

    // Clicking it applies the filter and pushes ?filter=no_cover into the URL
    // (HTMX swaps #browse-results; the chip bar itself lives outside the swap
    // target, same as every other home chip).
    await chip.click();
    await page.waitForURL(/[?&]filter=no_cover/);

    // A full load of the filtered URL renders the chip in its active state:
    // a status pill carrying a "remove filter" (×) control.
    await page.goto("/?filter=no_cover");
    const active = page.getByRole("status", { name: LABEL });
    await expect(active).toBeVisible();
    const clear = active.getByRole("button", { name: REMOVE });
    await expect(clear).toBeVisible();

    // Clearing removes the filter from the URL.
    await clear.click();
    await expect(page).not.toHaveURL(/[?&]filter=no_cover/);
  });

  test("anonymous: chip not rendered and ?filter=no_cover is ignored", async ({
    page,
  }) => {
    // Fresh context is anonymous — no login.
    await page.goto("/");
    await expect(page.getByRole("link", { name: LABEL })).toHaveCount(0);

    // Direct navigation to the filtered URL must not render the chip as active
    // (the filter is Librarian+ only; Anonymous gets no catalog-cleanup chip).
    await page.goto("/?filter=no_cover");
    await expect(page.getByRole("status", { name: LABEL })).toHaveCount(0);
    await expect(page.getByRole("link", { name: LABEL })).toHaveCount(0);
  });
});
