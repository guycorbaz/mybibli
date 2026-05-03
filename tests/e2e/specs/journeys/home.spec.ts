import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

test.describe("Home page", () => {
  test("should display mybibli title", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("h1")).toContainText("mybibli");
  });

  test("should have correct page title", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveTitle("mybibli");
  });

  test("should load Tailwind CSS styles", async ({ page }) => {
    await page.goto("/");
    const h1 = page.locator("h1");
    const color = await h1.evaluate((el) => getComputedStyle(el).color);
    // Indigo color should be applied (not default black)
    expect(color).not.toBe("rgb(0, 0, 0)");
  });
});

// Story 9-1 — "Collection at a glance" card.
test.describe("Home page — Collection at a glance card", () => {
  test("anonymous: card visible, three counts, loan count is NOT a link", async ({
    page,
  }) => {
    await page.goto("/");

    // Card section is present and labelled by its heading.
    const card = page.locator("#collection-glance");
    await expect(card).toBeVisible();
    await expect(
      card.getByRole("heading", { level: 2 }),
    ).toContainText(/Collection at a glance|Aperçu de la collection/i);

    // The three count rows are rendered IN ORDER (titles, volumes, loans) —
    // scope each assertion to its own <li> so a label/order swap regression
    // would fail (a global toContainText would silently match the wrong row).
    const rows = card.locator("ul > li");
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0)).toContainText(/\d+\s+(titles?|titres?)/i);
    await expect(rows.nth(1)).toContainText(/\d+\s+volumes?/i);
    await expect(rows.nth(2)).toContainText(
      /\d+\s+(active loans?|prêts? en cours)/i,
    );

    // CRITICAL: anonymous render must NOT leak the /loans link anywhere on the page.
    await expect(page.locator('a[href="/loans"]')).toHaveCount(0);

    // The aria-describedby target span exists (screen-reader sign-in hint),
    // and its anchor span carries the matching reference inside the card.
    await expect(card.locator("#glance-loans-hint")).toBeAttached();
    await expect(
      card.locator('[aria-describedby="glance-loans-hint"]'),
    ).toHaveCount(1);
  });

  test("librarian: card shows /loans link, click navigates to /loans", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    const card = page.locator("#collection-glance");
    await expect(card).toBeVisible();

    // The loan-count line is the third row and a real link to /loans
    // (exactly one inside the card; the nav bar also has one but that's
    // outside the card scope).
    const rows = card.locator("ul > li");
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(2)).toContainText(
      /\d+\s+(active loans?|prêts? en cours)/i,
    );

    const loansLink = card.locator('a[href="/loans"]');
    await expect(loansLink).toHaveCount(1);
    await expect(loansLink).toContainText(
      /active loans?|prêts? en cours/i,
    );

    await loansLink.click();
    await page.waitForURL(/\/loans/);
  });
});

// Story 9-2 — "Recent additions" section.
test.describe("Home page — Recent additions section", () => {
  test("anonymous: section visible, first card navigates to /title/:id (or empty-state shown)", async ({
    page,
  }) => {
    await page.goto("/");

    const section = page.locator("#recent-additions");
    await expect(section).toBeVisible();
    await expect(section.getByRole("heading", { level: 2 })).toContainText(
      /Recent additions|Ajouts récents/i,
    );

    const cards = section.locator("article.title-card");
    const count = await cards.count();

    if (count > 0) {
      // Populated catalog → click first card and verify navigation.
      await cards.first().click();
      await page.waitForURL(/\/title\/\d+/);
    } else {
      // Empty catalog → the inline empty-state is shown instead of hiding
      // the section (AC5).
      await expect(section).toContainText(
        /start cataloging|commencez à cataloguer/i,
      );
    }
  });
});

// Story 9-3 — "Stats by genre" section.
test.describe("Home page — Stats by genre section", () => {
  test("anonymous: section visible (or hidden), first row navigates to /?filter=genre:<id>", async ({
    page,
  }) => {
    await page.goto("/");

    const section = page.locator("#stats-by-genre");
    const sectionCount = await section.count();
    if (sectionCount === 0) {
      // AC4 — fresh catalog (zero genre assignments) hides the section
      // entirely. Nothing more to verify; the broader empty-catalog UX
      // belongs to story 9-15 (StatusMessage).
      return;
    }

    await expect(section).toBeVisible();
    await expect(section.getByRole("heading", { level: 2 })).toContainText(
      /By genre|Par genre/i,
    );

    // The first row must show a percentage in either EN (33.3%) or FR
    // (33,3 %) format. Combined regex accepts both.
    const rows = section.locator("li");
    await expect(rows.first()).toContainText(/\d+([.,]\d+)?\s*%/);

    // Scoped selector — explicitly avoids the unscoped-selector flake
    // class flagged by 9-2's review (a global a[href^="/?filter=genre:"]
    // would also match the genre-filter pills above #browse-results).
    const firstLink = section.locator('a[href^="/?filter=genre:"]').first();
    await expect(firstLink).toBeVisible();
    await firstLink.click();
    await page.waitForURL(/\/\?filter=genre%3A\d+|\/\?filter=genre:\d+/);
  });
});

// Story 9-4 — "What needs attention" / Unshelved indicator.
test.describe("Home page — What needs attention / Unshelved indicator", () => {
  test("anonymous: section not rendered, indicator filter param ignored", async ({
    page,
  }) => {
    await page.goto("/");
    // AC2 — anonymous never sees the section.
    await expect(page.locator("#what-needs-attention")).toHaveCount(0);
    await expect(page.locator("#filter-tag-unshelved")).toHaveCount(0);

    // Anonymous crafting `?filter=unshelved` — filter is ignored, no leak.
    await page.goto("/?filter=unshelved");
    await expect(page.locator("#what-needs-attention")).toHaveCount(0);
    await expect(page.locator("#unshelved-list")).toHaveCount(0);
    // The default home (recent-additions) is still visible.
    await expect(page.locator("#recent-additions")).toBeVisible();
  });

  test("librarian: tag visible, click → unshelved-list, ✕ → home", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    const section = page.locator("#what-needs-attention");
    const sectionCount = await section.count();
    if (sectionCount === 0) {
      // Seed DB has zero unshelved volumes — section is hidden by AC3
      // zero-count rule. Same defensive empty-DB short-circuit pattern
      // as 9-2/9-3 E2E tests.
      return;
    }

    await expect(section).toBeVisible();
    await expect(section.getByRole("heading", { level: 2 })).toContainText(
      /What needs attention|À traiter/i,
    );
    const tag = page.locator("#filter-tag-unshelved");
    await expect(tag).toBeVisible();
    // Default state — href targets the indicator filter URL.
    await expect(tag).toHaveAttribute("href", "/?filter=unshelved");

    // Click the tag → URL changes → unshelved-list replaces recent-additions.
    await tag.click();
    await page.waitForURL(/\/\?filter=unshelved/);
    await expect(page.locator("#unshelved-list")).toBeVisible();
    await expect(page.locator("#recent-additions")).toHaveCount(0);

    // The tag is now in active state — href clears the filter.
    const activeTag = page.locator("#filter-tag-unshelved");
    await expect(activeTag).toHaveAttribute("href", "/");

    // Click ✕ → URL returns → recent-additions back, unshelved-list gone.
    await activeTag.click();
    await page.waitForURL(/\/$/);
    await expect(page.locator("#recent-additions")).toBeVisible();
    await expect(page.locator("#unshelved-list")).toHaveCount(0);
  });
});

// Story 9-5 — "What needs attention" / Overdue loans indicator.
test.describe("Home page — Overdue loans indicator", () => {
  test("anonymous: tag not rendered, indicator filter param ignored", async ({
    page,
  }) => {
    await page.goto("/");
    // AC2 — anonymous never sees the overdue tag or list.
    await expect(page.locator("#filter-tag-overdue")).toHaveCount(0);
    await expect(page.locator("#overdue-list")).toHaveCount(0);

    // Anonymous crafting `?filter=overdue` — filter is ignored, no leak.
    await page.goto("/?filter=overdue");
    await expect(page.locator("#filter-tag-overdue")).toHaveCount(0);
    await expect(page.locator("#overdue-list")).toHaveCount(0);
    // The default home (recent-additions) is still visible.
    await expect(page.locator("#recent-additions")).toBeVisible();
  });

  test("librarian: tag visible iff count > 0; click → overdue-list, ✕ → home", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    // Conditional empty-DB short-circuit — same defensive pattern as
    // the 9-4 unshelved smoke test. Seed DB may or may not contain
    // overdue loans depending on fixture freshness.
    const tag = page.locator("#filter-tag-overdue");
    const tagCount = await tag.count();
    if (tagCount === 0) {
      // No overdue loans seeded — AC3 zero-count rule hides the tag.
      return;
    }

    await expect(tag).toBeVisible();
    // Default state — href targets the indicator filter URL.
    await expect(tag).toHaveAttribute("href", "/?filter=overdue");

    // Click the tag → URL changes → overdue-list replaces recent-additions.
    await tag.click();
    await page.waitForURL(/\/\?filter=overdue/);
    await expect(page.locator("#overdue-list")).toBeVisible();
    // 3-way mutual exclusion (AC6).
    await expect(page.locator("#recent-additions")).toHaveCount(0);
    await expect(page.locator("#unshelved-list")).toHaveCount(0);

    // The tag is now in active state — href clears the filter.
    const activeTag = page.locator("#filter-tag-overdue");
    await expect(activeTag).toHaveAttribute("href", "/");

    // Click ✕ → URL returns → recent-additions back, overdue-list gone.
    await activeTag.click();
    await page.waitForURL(/\/$/);
    await expect(page.locator("#recent-additions")).toBeVisible();
    await expect(page.locator("#overdue-list")).toHaveCount(0);
  });
});

// Story 9-6 — "What needs attention" / Series with gaps indicator.
// AC2 LOAD-BEARING asymmetry vs 9-4/9-5: anonymous CAN navigate to
// /?filter=gaps and see the list (series browsing is anonymous-allowed
// per FR65 + FR95) — but the TAG itself is still hidden from anonymous
// on the default home (where #what-needs-attention requires Librarian).
test.describe("Home page — Series with gaps indicator", () => {
  test("anonymous: tag never rendered, BUT /?filter=gaps shows the list (AC2 asymmetry)", async ({
    page,
  }) => {
    await page.goto("/");
    // Default home: no tag (Librarian-only section), no list.
    await expect(page.locator("#filter-tag-gaps")).toHaveCount(0);
    await expect(page.locator("#gaps-list")).toHaveCount(0);

    // Load-bearing AC2: anonymous + ?filter=gaps → #gaps-list IS rendered
    // (anonymous-allowed asymmetry vs unshelved/overdue) AND
    // #filter-tag-gaps is NOT (no tag for anonymous on /).
    await page.goto("/?filter=gaps");
    await expect(page.locator("#filter-tag-gaps")).toHaveCount(0);
    await expect(page.locator("#gaps-list")).toHaveCount(1);
    // Mutual exclusion: gaps-list replaces recent-additions for anonymous too.
    await expect(page.locator("#recent-additions")).toHaveCount(0);
  });

  test("librarian: tag visible iff count > 0; click → gaps-list, ✕ → home", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    // Conditional empty-DB short-circuit — same defensive pattern as the
    // 9-4 unshelved + 9-5 overdue smoke tests. Seed DB may or may not
    // contain gappy series depending on fixture freshness.
    const tag = page.locator("#filter-tag-gaps");
    const tagCount = await tag.count();
    if (tagCount === 0) {
      // No gappy closed series seeded — AC3 zero-count rule hides the tag.
      return;
    }

    await expect(tag).toBeVisible();
    await expect(tag).toHaveAttribute("href", "/?filter=gaps");

    // Click the tag → URL changes → gaps-list replaces recent-additions.
    await tag.click();
    await page.waitForURL(/\/\?filter=gaps/);
    await expect(page.locator("#gaps-list")).toBeVisible();
    // AC6 4-way mutual exclusion.
    await expect(page.locator("#recent-additions")).toHaveCount(0);
    await expect(page.locator("#unshelved-list")).toHaveCount(0);
    await expect(page.locator("#overdue-list")).toHaveCount(0);

    // The tag is now in active state — href clears the filter.
    const activeTag = page.locator("#filter-tag-gaps");
    await expect(activeTag).toHaveAttribute("href", "/");

    // Click ✕ → URL returns → recent-additions back, gaps-list gone.
    await activeTag.click();
    await page.waitForURL(/\/$/);
    await expect(page.locator("#recent-additions")).toBeVisible();
    await expect(page.locator("#gaps-list")).toHaveCount(0);
  });
});

// Story 9-7 — "What needs attention" / Recent activity indicators
// (recent_cataloged + recent_returns). Closes the indicator-subsystem
// chapter at 5/5 indicators. Symmetric Librarian-only role gating
// (NOT 9-6's Gaps anonymous-allowed asymmetry).
test.describe("Home page — Recent activity indicators", () => {
  test("anonymous: tags + lists never rendered (symmetric Librarian-gated)", async ({
    page,
  }) => {
    // Default home: no tags, no list sections.
    await page.goto("/");
    await expect(page.locator("#filter-tag-recent-cataloged")).toHaveCount(0);
    await expect(page.locator("#filter-tag-recent-returns")).toHaveCount(0);
    await expect(page.locator("#recent-cataloged-list")).toHaveCount(0);
    await expect(page.locator("#recent-returns-list")).toHaveCount(0);

    // Anonymous + ?filter=recent-cataloged → ignored, default home renders.
    await page.goto("/?filter=recent-cataloged");
    await expect(page.locator("#filter-tag-recent-cataloged")).toHaveCount(0);
    await expect(page.locator("#recent-cataloged-list")).toHaveCount(0);
    await expect(page.locator("#recent-additions")).toBeVisible();

    // Anonymous + ?filter=recent-returns → same.
    await page.goto("/?filter=recent-returns");
    await expect(page.locator("#filter-tag-recent-returns")).toHaveCount(0);
    await expect(page.locator("#recent-returns-list")).toHaveCount(0);
    await expect(page.locator("#recent-additions")).toBeVisible();
  });

  test("librarian: each tag visible iff count > 0; click → list, ✕ → home (covers BOTH indicators)", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    // Conditional empty-DB short-circuit. If BOTH counts are 0, return
    // green pass — same defensive pattern as 9-4/9-5/9-6 librarian smokes.
    const cataloged = page.locator("#filter-tag-recent-cataloged");
    const returns = page.locator("#filter-tag-recent-returns");
    const catalogedCount = await cataloged.count();
    const returnsCount = await returns.count();
    if (catalogedCount === 0 && returnsCount === 0) {
      // No recent activity in seed DB.
      return;
    }

    // Exercise recent-cataloged tag if present.
    if (catalogedCount === 1) {
      await expect(cataloged).toBeVisible();
      await expect(cataloged).toHaveAttribute("href", "/?filter=recent-cataloged");

      await cataloged.click();
      await page.waitForURL(/\/\?filter=recent-cataloged/);
      await expect(page.locator("#recent-cataloged-list")).toBeVisible();
      // 6-way mutual exclusion.
      await expect(page.locator("#recent-additions")).toHaveCount(0);
      await expect(page.locator("#unshelved-list")).toHaveCount(0);
      await expect(page.locator("#overdue-list")).toHaveCount(0);
      await expect(page.locator("#gaps-list")).toHaveCount(0);
      await expect(page.locator("#recent-returns-list")).toHaveCount(0);

      // Active state ✕ → URL returns → recent-additions back.
      const activeTag = page.locator("#filter-tag-recent-cataloged");
      await expect(activeTag).toHaveAttribute("href", "/");
      await activeTag.click();
      await page.waitForURL(/\/$/);
      await expect(page.locator("#recent-additions")).toBeVisible();
      await expect(page.locator("#recent-cataloged-list")).toHaveCount(0);
    }

    // Exercise recent-returns tag if present.
    if (returnsCount === 1) {
      await expect(returns).toBeVisible();
      await expect(returns).toHaveAttribute("href", "/?filter=recent-returns");

      await returns.click();
      await page.waitForURL(/\/\?filter=recent-returns/);
      await expect(page.locator("#recent-returns-list")).toBeVisible();
      // 6-way mutual exclusion.
      await expect(page.locator("#recent-additions")).toHaveCount(0);
      await expect(page.locator("#unshelved-list")).toHaveCount(0);
      await expect(page.locator("#overdue-list")).toHaveCount(0);
      await expect(page.locator("#gaps-list")).toHaveCount(0);
      await expect(page.locator("#recent-cataloged-list")).toHaveCount(0);

      const activeTag = page.locator("#filter-tag-recent-returns");
      await expect(activeTag).toHaveAttribute("href", "/");
      await activeTag.click();
      await page.waitForURL(/\/$/);
      await expect(page.locator("#recent-additions")).toBeVisible();
      await expect(page.locator("#recent-returns-list")).toHaveCount(0);
    }
  });
});
