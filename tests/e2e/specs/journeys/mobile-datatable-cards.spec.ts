/**
 * Story 10-3 (closes #159) — Mobile DataTable card mode.
 *
 * `/loans`, `/borrowers` and `/borrower/:id` now render the same row
 * data twice: a desktop table (`hidden md:block`) and a mobile-card
 * list (`md:hidden`). At the mobile breakpoint (< 768px) only the
 * cards are visible; at md+ only the table is.
 *
 * Spec ID "MC" — never scans an ISBN in this file but the convention
 * stands for any future addition.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import {
  scanTitleAndVolume,
  createBorrower,
  createLoan,
} from "../../helpers/loans";
import { specIsbn } from "../../helpers/isbn";

const MOBILE_VIEWPORT = { width: 375, height: 667 };

test.describe("Story 10-3 — mobile DataTable card mode", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("/loans renders cards on mobile, table on desktop (same loan in both)", async ({
    page,
  }) => {
    // Seed: borrower + title + volume + loan.
    const isbn = specIsbn("MC", 1);
    const volumeLabel = "V8001";
    const borrowerName = "MC-Loans-User-1";
    await scanTitleAndVolume(page, isbn, volumeLabel);
    await createBorrower(page, borrowerName);
    await createLoan(page, volumeLabel, borrowerName);

    // Mobile viewport — cards visible, table hidden.
    await page.setViewportSize(MOBILE_VIEWPORT);
    await page.goto("/loans");
    const mobileCards = page.locator("#loans-cards-mobile");
    await expect(mobileCards).toBeVisible();
    await expect(
      mobileCards.locator("article", { hasText: borrowerName }),
    ).toBeVisible();
    await expect(page.locator("#loans-table-body")).not.toBeVisible();

    // Desktop viewport — table visible, cards hidden.
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.reload();
    await expect(page.locator("#loans-table-body")).toBeVisible();
    await expect(page.locator("#loans-cards-mobile")).not.toBeVisible();
  });

  test("/borrowers renders mobile cards on small viewport", async ({
    page,
  }) => {
    await createBorrower(page, "MC-Borrower-User-2");
    await page.setViewportSize(MOBILE_VIEWPORT);
    await page.goto("/borrowers");
    const mobileCards = page.locator("#borrowers-cards-mobile");
    await expect(mobileCards).toBeVisible();
    await expect(
      mobileCards.locator("article", { hasText: "MC-Borrower-User-2" }),
    ).toBeVisible();
  });

  test("/borrower/:id renders the active-loans mobile cards", async ({
    page,
  }) => {
    // Seed: borrower + title + volume + loan
    const isbn = specIsbn("MC", 3);
    const volumeLabel = "V8003";
    const borrowerName = "MC-Borrower-Detail-User-3";
    await scanTitleAndVolume(page, isbn, volumeLabel);
    await createBorrower(page, borrowerName);
    await createLoan(page, volumeLabel, borrowerName);

    // Resolve the borrower's detail URL via the /borrowers list
    await page.goto("/borrowers");
    const link = page.locator(`a:has-text("${borrowerName}")`).first();
    const href = await link.getAttribute("href");
    expect(href).toMatch(/\/borrower\/\d+/);

    await page.setViewportSize(MOBILE_VIEWPORT);
    await page.goto(href!);

    const mobileCards = page.locator("#borrower-loans-cards-mobile");
    await expect(mobileCards).toBeVisible();
    await expect(
      mobileCards.locator("article", { hasText: volumeLabel }),
    ).toBeVisible();
  });
});
