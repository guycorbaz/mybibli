import { test, expect, Page } from "@playwright/test";
import { execSync } from "child_process";
import { loginAs, logout } from "../../helpers/auth";

/**
 * E2E tests for Story 5-7: Similar Titles Section on /title/:id.
 *
 * Scope:
 *   - Test 1 — Similar titles by shared contributor (AC #1, #5, #6, #7, #14)
 *   - Test 2 — Anonymous public read (AC #12)
 *
 * Why this spec uses manual title creation instead of ISBN scanning:
 *   The mock BnF catch-all at tests/e2e/mock-metadata-server/server.py attaches
 *   a default "Synthetic TestAuthor" contributor + publication_date=2024 to
 *   every scanned title. That shared state makes cross-spec bleed via arm 2
 *   (contributor) of find_similar and ruins any strict "exactly 2" count
 *   assertion.
 *
 *   By going through POST /catalog/title (manual title creation) no metadata
 *   task runs, no default contributor is attached, and the only contributor on
 *   the anchor is the explicit one we choose. This lets us use STRICT
 *   toHaveLength(2) assertions — the path spec Task 6.5 prescribed as the
 *   non-fallback option.
 *
 *   AC #3 (absent section when empty) is still covered by unit tests in
 *   src/routes/titles.rs (test_title_detail_template_renders asserts no
 *   <section> for empty Vec).
 */

const AUTEUR_ROLE_ID = "1"; // seeded via migrations/20260330000002_seed_default_reference_data.sql
const ROMAN_GENRE_ID = "1"; // seeded via migrations/20260330000001_seed_default_genres.sql

async function createTitleManually(page: Page, name: string): Promise<void> {
  const response = await page.request.post("/catalog/title", {
    form: {
      title: name,
      media_type: "book",
      genre_id: ROMAN_GENRE_ID,
      language: "fr",
    },
  });
  if (!response.ok()) {
    throw new Error(
      `Failed to create title "${name}": ${response.status()} ${await response.text()}`,
    );
  }
}

/**
 * Look up the latest title id by exact title name via direct DB access.
 *
 * The home search uses a FULLTEXT BOOLEAN MODE match with `ORDER BY t.title`,
 * which ranks results alphabetically rather than by relevance — so searching
 * for "ST Similar Title Alpha 2026" returns "ST Anon Title Alpha 2026" first
 * when both exist in parallel-run state. Using direct DB access is the
 * "fixture script" fallback the story spec explicitly allows for this kind of
 * deterministic lookup (Task 6.5).
 */
function getTitleIdByExactName(name: string): string {
  const escaped = name.replace(/'/g, "''");
  const sql = `SELECT id FROM titles WHERE title='${escaped}' AND deleted_at IS NULL ORDER BY id DESC LIMIT 1;`;
  const out = execSync(
    `docker exec e2e-db-1 mariadb -u mybibli -pmybibli_test mybibli_test -BNe "${sql}"`,
    { encoding: "utf8" },
  ).trim();
  if (!out) {
    throw new Error(`Title not found by exact name: ${name}`);
  }
  return out;
}

async function addContributorToTitle(
  page: Page,
  titleId: string,
  contributorName: string,
): Promise<void> {
  const response = await page.request.post("/catalog/contributors/add", {
    form: {
      title_id: titleId,
      contributor_name: contributorName,
      role_id: AUTEUR_ROLE_ID,
    },
  });
  if (!response.ok()) {
    throw new Error(
      `Failed to add contributor "${contributorName}" to title ${titleId}: ${response.status()}`,
    );
  }
}

test.describe("Similar titles section (Story 5-7)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("shows related titles sharing the same contributor", async ({
    page,
  }) => {
    // Unique contributor name isolates this spec from cross-spec noise.
    const sharedContributor = "ST Shared Author 2026";
    const titleNames = [
      "ST Similar Title Alpha 2026",
      "ST Similar Title Beta 2026",
      "ST Similar Title Gamma 2026",
    ];

    // Create 3 titles manually (no metadata fetch → no Synthetic TestAuthor)
    for (const name of titleNames) {
      await createTitleManually(page, name);
    }

    // Capture the title IDs via direct DB lookup (deterministic)
    const ids = titleNames.map(getTitleIdByExactName);
    const [id1, id2, id3] = ids;

    // Attach the same explicit contributor to each — the only contributor on any of them
    for (const id of ids) {
      await addContributorToTitle(page, id, sharedContributor);
    }

    // Navigate to title #1 and assert the Similar titles section
    await page.goto(`/title/${id1}`);
    await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });

    const section = page.locator(
      'section[aria-label="Similar titles"], section[aria-label="Titres similaires"]',
    );
    await expect(section).toBeVisible({ timeout: 5000 });

    const hrefs = await section
      .locator('a[href^="/title/"]')
      .evaluateAll((els) =>
        els.map((e) => (e as HTMLAnchorElement).getAttribute("href")!),
      );

    // AC #1, #6 — STRICT assertion: exactly the 2 sibling titles appear, no noise
    expect(hrefs).toHaveLength(2);
    expect(hrefs).toEqual(
      expect.arrayContaining([`/title/${id2}`, `/title/${id3}`]),
    );

    // AC #5 — current title is never its own similar
    expect(hrefs).not.toContain(`/title/${id1}`);

    // AC #7 — clicking a similar title navigates to that title's detail page
    const firstSimilar = section.locator('a[href^="/title/"]').first();
    const targetHref = await firstSimilar.getAttribute("href");
    await firstSimilar.click();
    await expect(page).toHaveURL(new RegExp(`${targetHref}$`), {
      timeout: 5000,
    });
    await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });
    // Wikipedia-effect chain: destination title also has a similar section
    await expect(
      page.locator(
        'section[aria-label="Similar titles"], section[aria-label="Titres similaires"]',
      ),
    ).toBeVisible({ timeout: 5000 });
  });

  test("anonymous users can see the similar titles section (public read, FR95)", async ({
    page,
  }) => {
    const anonContributor = "ST Anon Author 2026";
    const titleNames = [
      "ST Anon Title Alpha 2026",
      "ST Anon Title Beta 2026",
      "ST Anon Title Gamma 2026",
    ];

    for (const name of titleNames) {
      await createTitleManually(page, name);
    }
    const ids = titleNames.map(getTitleIdByExactName);
    const [idA] = ids;
    for (const id of ids) {
      await addContributorToTitle(page, id, anonContributor);
    }

    // Clear session cookies to simulate an anonymous user
    await logout(page);

    // Navigate directly to the title detail page without logging in
    await page.goto(`/title/${idA}`);
    await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });

    // AC #12 — Similar titles section is visible to anonymous users
    const section = page.locator(
      'section[aria-label="Similar titles"], section[aria-label="Titres similaires"]',
    );
    await expect(section).toBeVisible({ timeout: 5000 });

    const hrefs = await section
      .locator('a[href^="/title/"]')
      .evaluateAll((els) =>
        els.map((e) => (e as HTMLAnchorElement).getAttribute("href")!),
      );

    // Strict assertion: the 2 siblings are visible to anonymous users
    expect(hrefs).toHaveLength(2);
    expect(hrefs).toEqual(
      expect.arrayContaining([`/title/${ids[1]}`, `/title/${ids[2]}`]),
    );
    expect(hrefs).not.toContain(`/title/${idA}`);
  });
});
