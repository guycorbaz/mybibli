import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { createLocation } from "../../helpers/locations";
import { specVcode, vcodeSlot } from "../../helpers/vcode";

/**
 * Issue #428 — "labels in use up to V…· L…" info line on /catalog, so
 * freshly printed label sheets start after the highest used numbers.
 *
 * Rule #23 (#554 item 2): the line shows the library-wide maximum, which
 * any parallel spec can move — no code is "reserved" by convention. The
 * spec therefore asserts only on its own rows: the codes on the line are
 * parsed and must be at least the ones it created (fixed-width `V`/`L` + 4
 * digits, so a string comparison is a numeric one). Its V-code comes from
 * the top `specVcode` block (`HC`, V9800–V9999), the highest any spec can
 * create, which keeps the check meaningful; its L-code is L9998.
 */
test.describe("Issue #428 — highest V/L-code line on /catalog", () => {
  test("librarian sees the high-water marks after creating them", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    const vcode = specVcode("HC", 0);
    const lcode = "L9998";
    // A fresh ISBN per attempt slot: the code is new each time, the title
    // must be too (an existing title with volumes takes the phantom-volume
    // path). The location creation is not rerun-safe (UNIQUE L-code on a
    // persistent DB), so only create it when an earlier attempt hasn't.
    await scanTitleAndVolume(page, specIsbn("HC", 1 + vcodeSlot()), vcode);
    await page.goto("/locations");
    const existing = page.locator(`text=${lcode}`);
    if (!(await existing.isVisible({ timeout: 2000 }).catch(() => false))) {
      await createLocation(page, "HC-428 shelf", lcode);
    }

    await page.goto("/catalog");
    const line = page.locator("#highest-codes-line");
    await expect(line).toBeVisible();
    const text = (await line.textContent()) ?? "";
    const shownV = text.match(/\bV\d{4}\b/)?.[0];
    const shownL = text.match(/\bL\d{4}\b/)?.[0];
    expect(shownV, `a V-code on the line: "${text}"`).toBeTruthy();
    expect(shownL, `an L-code on the line: "${text}"`).toBeTruthy();
    expect(shownV! >= vcode, `${shownV} is at least ${vcode}`).toBe(true);
    expect(shownL! >= lcode, `${shownL} is at least ${lcode}`).toBe(true);
  });

  test("anonymous visitor does not see the line", async ({ page }) => {
    // No login — /catalog is anonymous-accessible but the label-printing
    // line is a Librarian+ affordance.
    await page.goto("/catalog");
    // Page rendered (guide strip is anonymous-visible; the scan field is
    // itself a Librarian+ affordance) — and the label line is absent.
    await expect(page.locator("#guide-strip")).toBeVisible();
    await expect(page.locator("#highest-codes-line")).toHaveCount(0);
  });
});
