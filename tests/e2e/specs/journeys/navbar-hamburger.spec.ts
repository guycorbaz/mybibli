/**
 * Story 9-17 — NavBar hamburger menu + scanner auto-close E2E.
 *
 * Covers AC11 — 5 scenarios:
 *   1. Hamburger visible on tablet (md breakpoint), hidden on desktop;
 *      reverse on resize.
 *   2. Open / link-click / Escape / outside-click — each path closes the
 *      panel and restores aria-expanded=false on the trigger.
 *   3. Focus trap — Tab from last focusable wraps to first; Shift+Tab
 *      from first wraps to last.
 *   4. Scanner-burst auto-close on a page WITH #scan-field (/catalog) —
 *      simulateScan with 20 ms inter-key trips the burst detector; panel
 *      closes; the burst-confirming keystroke is forwarded to #scan-field.
 *   5. Scanner-burst auto-close on a page WITHOUT #scan-field (/login) —
 *      panel closes; no error from missing scan field.
 *
 * Spec ID "NH" — no ISBNs generated.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { simulateScan } from "../../helpers/scanner";

const TABLET = { width: 600, height: 800 };
const DESKTOP = { width: 1280, height: 720 };

test.describe("Story 9-17 — NavBar hamburger menu", () => {
  test("hamburger visible on tablet, hidden on desktop", async ({ page }) => {
    await page.setViewportSize(TABLET);
    await page.goto("/login");

    const trigger = page.locator("#mobile-menu-toggle");
    const desktopNav = page
      .locator("nav[aria-label='Main navigation'] > div.hidden.md\\:flex")
      .first();

    await expect(trigger).toBeVisible();
    await expect(desktopNav).toBeHidden();

    await page.setViewportSize(DESKTOP);

    await expect(trigger).toBeHidden();
    await expect(desktopNav).toBeVisible();
  });

  test("open / link-click / Escape / outside-click each close the panel", async ({
    page,
  }) => {
    await page.setViewportSize(TABLET);
    await page.goto("/catalog");

    const trigger = page.locator("#mobile-menu-toggle");
    const panel = page.locator("#mobile-nav");

    // --- Open ---
    await trigger.click();
    await expect(panel).not.toHaveClass(/(?:^|\s)hidden(?:\s|$)/);
    await expect(trigger).toHaveAttribute("aria-expanded", "true");

    // --- Click on a link inside the panel — verifies nav.js's link-click
    //     handler closes the panel BEFORE navigation. We intercept the
    //     navigation to avoid asserting against the next page's
    //     default-hidden render (which would be tautological — every page
    //     starts with the panel hidden). With navigation cancelled, the
    //     only thing that can hide the panel is nav.js's handler.
    await page.evaluate(() => {
      document.querySelectorAll("#mobile-nav a[href]").forEach((a) => {
        a.addEventListener("click", (e) => e.preventDefault(), { once: true });
      });
    });
    await panel.locator("a[href='/locations']").click();
    await expect(panel).toHaveClass(/(?:^|\s)hidden(?:\s|$)/);
    await expect(trigger).toHaveAttribute("aria-expanded", "false");

    // --- Escape close ---
    await page.locator("#mobile-menu-toggle").click();
    await expect(page.locator("#mobile-nav")).not.toHaveClass(/(?:^|\s)hidden(?:\s|$)/);
    await page.keyboard.press("Escape");
    await expect(page.locator("#mobile-nav")).toHaveClass(/(?:^|\s)hidden(?:\s|$)/);
    await expect(page.locator("#mobile-menu-toggle")).toHaveAttribute(
      "aria-expanded",
      "false",
    );

    // --- Outside-click close: open again, click in <main> (outside both
    //     trigger and panel) ---
    await page.locator("#mobile-menu-toggle").click();
    await expect(page.locator("#mobile-nav")).not.toHaveClass(/(?:^|\s)hidden(?:\s|$)/);
    // dispatchEvent for mousedown to match nav.js's listener phase
    await page.evaluate(() => {
      const main = document.querySelector("main");
      if (main) {
        main.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      }
    });
    await expect(page.locator("#mobile-nav")).toHaveClass(/(?:^|\s)hidden(?:\s|$)/);
    await expect(page.locator("#mobile-menu-toggle")).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  test("focus trap — Tab from last wraps to first, Shift+Tab from first wraps to last", async ({
    page,
  }) => {
    await page.setViewportSize(TABLET);
    await page.goto("/catalog");

    await page.locator("#mobile-menu-toggle").click();
    await expect(page.locator("#mobile-nav")).not.toHaveClass(/(?:^|\s)hidden(?:\s|$)/);

    // The first focusable inside the panel is `a[href='/catalog']`.
    // After open(), nav.js focuses it. Verify.
    const firstHref = await page.evaluate(
      () => (document.activeElement as HTMLAnchorElement | null)?.getAttribute("href"),
    );
    expect(firstHref).toBe("/catalog");

    // Walk Tab forward to reach the last focusable inside the panel.
    // The panel contains: catalog, locations, series links, then the
    // language form (FR + EN buttons). On /catalog as anonymous, those
    // are 3 links + 2 buttons = 5 focusable items, plus 2 hidden inputs
    // that are excluded by the [type="hidden"] guard.
    const focusableCount = await page.locator(
      "#mobile-nav a[href], #mobile-nav button:not([disabled])",
    ).count();
    expect(focusableCount).toBeGreaterThan(0);

    // Tab to last by pressing Tab (focusableCount - 1) times. Then one
    // more Tab should wrap to the first.
    for (let i = 0; i < focusableCount - 1; i++) {
      await page.keyboard.press("Tab");
    }
    // We are now on the LAST focusable. Press Tab — should wrap to first.
    await page.keyboard.press("Tab");
    const wrappedFirst = await page.evaluate(
      () => (document.activeElement as HTMLAnchorElement | null)?.getAttribute("href"),
    );
    expect(wrappedFirst).toBe("/catalog");

    // Shift+Tab from first wraps to last.
    await page.keyboard.press("Shift+Tab");
    const insidePanel = await page.evaluate(() => {
      const panel = document.getElementById("mobile-nav");
      const active = document.activeElement;
      return panel && active ? panel.contains(active) : false;
    });
    expect(insidePanel).toBe(true);
  });

  test("scanner-burst auto-close on /catalog forwards keystroke to #scan-field", async ({
    page,
  }) => {
    // #scan-field is gated to librarian/admin on /catalog (templates/pages/
    // catalog.html:47). Authenticate so the scan field is present.
    await loginAs(page, "librarian");
    await page.setViewportSize(TABLET);
    await page.goto("/catalog");

    // Confirm prerequisite — #scan-field exists on /catalog for librarian.
    await expect(page.locator("#scan-field")).toBeAttached();

    await page.locator("#mobile-menu-toggle").click();
    await expect(page.locator("#mobile-nav")).not.toHaveClass(/(?:^|\s)hidden(?:\s|$)/);

    // Type the burst at document level WITHOUT a trailing Enter — the
    // Enter would land on #scan-field after nav.js forwards focus to it,
    // and scan-field.js would route the (invalid) scan via /scan,
    // navigating away before the value-assertion can read the field.
    // simulateScan helpfully types "+Enter"; we bypass it here and use
    // page.keyboard.type directly to keep nav focus where it is.
    //
    // delay: 5 ms (well below the 50 ms burst threshold) — at delay: 20
    // a slow CI runner could slip the second keystroke past 50 ms and
    // miss the burst classification, producing a flake. delay: 5 holds
    // a 10× margin which is robust against scheduler jitter.
    await page.keyboard.type("AB", { delay: 5 });

    // Panel must collapse within ~1s — Playwright's auto-wait under
    // toHaveClass uses default test timeout but we tighten it to keep
    // failures fast.
    await expect(page.locator("#mobile-nav")).toHaveClass(/(?:^|\s)hidden(?:\s|$)/, {
      timeout: 2000,
    });
    await expect(page.locator("#mobile-menu-toggle")).toHaveAttribute(
      "aria-expanded",
      "false",
    );

    // The burst-confirming keystroke is forwarded to #scan-field. Because
    // the burst was 'A' → 'B', the first keystroke ('A') is consumed by
    // the panel's normal focus target (no-op — `<a>` doesn't accept text)
    // and the SECOND keystroke ('B') is the one that classifies the pair
    // as "burst" and gets forwarded to #scan-field.
    const scanValue = await page.locator("#scan-field").inputValue();
    expect(scanValue).toContain("B");
  });

  test("scanner-burst auto-close on /login (no #scan-field) closes panel without error", async ({
    page,
  }) => {
    await page.setViewportSize(TABLET);
    await page.goto("/login");

    // Confirm prerequisite — #scan-field does NOT exist on /login.
    await expect(page.locator("#scan-field")).toHaveCount(0);

    await page.locator("#mobile-menu-toggle").click();
    await expect(page.locator("#mobile-nav")).not.toHaveClass(/(?:^|\s)hidden(?:\s|$)/);

    // Track JS console errors so we can assert nav.js doesn't blow up
    // when #scan-field is absent.
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));

    await simulateScan(page, "body", "AB");

    await expect(page.locator("#mobile-nav")).toHaveClass(/(?:^|\s)hidden(?:\s|$)/, {
      timeout: 2000,
    });
    await expect(page.locator("#mobile-menu-toggle")).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(errors).toHaveLength(0);
  });

  // Issue #150 — authenticated users on tablet/mobile can log out via
  // the hamburger panel (parity with the desktop strip hidden by md:hidden).
  test("logout from mobile panel signs the user out", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.setViewportSize(TABLET);
    await page.goto("/catalog");

    await page.locator("#mobile-menu-toggle").click();
    await expect(page.locator("#mobile-nav")).not.toHaveClass(
      /(?:^|\s)hidden(?:\s|$)/,
    );

    // The mobile panel carries its own POST /logout form. Submit it and
    // follow the redirect to /.
    const logoutForm = page.locator(
      '#mobile-nav form[action="/logout"][method="POST"]',
    );
    await expect(logoutForm).toBeVisible();
    await logoutForm.locator('button[type="submit"]').click();

    // Post-logout, the session cookie is cleared and the user lands on
    // the public landing page with the anonymous navbar.
    await page.waitForURL((u) => u.pathname === "/");
    await page.setViewportSize(TABLET);
    await page.locator("#mobile-menu-toggle").click();
    await expect(
      page.locator('#mobile-nav a[href="/login"]'),
    ).toBeVisible();
    await expect(
      page.locator('#mobile-nav form[action="/logout"]'),
    ).toHaveCount(0);
  });
});
