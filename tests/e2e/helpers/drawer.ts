import { Page, Locator, expect } from "@playwright/test";

/**
 * Story 14-2 (#547, UX-DR39) — the small-screen navigation drawer.
 *
 * Below `lg` (1024 px) the sidebar's `<nav id="sidebar">` is an off-canvas
 * drawer opened by the bar's menu button `#mobile-menu-toggle`; while open
 * its wrapper `#nav-drawer` is a modal dialog (`role="dialog"`,
 * `aria-modal="true"`). Closed, the nav is `invisible` (not visible to
 * Playwright, not focusable).
 */
export const MENU_BUTTON = "#mobile-menu-toggle";
export const DRAWER = "#sidebar";
export const DRAWER_DIALOG = "#nav-drawer";
export const DRAWER_CLOSE = "#nav-drawer-close";
export const SCRIM = "#nav-scrim";

/** Opens the drawer from the menu button and waits until it is in place. */
export async function openDrawer(page: Page): Promise<Locator> {
  await page.locator(MENU_BUTTON).click();
  await expectDrawerOpen(page);
  return page.locator(DRAWER);
}

export async function expectDrawerOpen(page: Page): Promise<void> {
  await expect(page.locator(MENU_BUTTON)).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator(DRAWER_DIALOG)).toHaveAttribute("aria-modal", "true");
  await expect(page.locator(DRAWER)).toBeVisible();
  // The slide is over (no transition under reduced motion; otherwise wait
  // until the drawer's left edge is at the viewport's left edge).
  await expect
    .poll(() => page.locator(DRAWER).evaluate((el) => Math.round(el.getBoundingClientRect().left)))
    .toBe(0);
}

export async function expectDrawerClosed(page: Page): Promise<void> {
  await expect(page.locator(MENU_BUTTON)).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator(DRAWER_DIALOG)).not.toHaveAttribute("aria-modal", "true");
  await expect(page.locator(DRAWER)).toBeHidden();
}
