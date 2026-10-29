/**
 * Story 11-15 E2E — proposal history and resubmission delay (FR135,
 * FR136, AR30).
 *
 * Journey 1, from a blank browser: loginAs(admin) → Admin → System →
 * the Proposals section shows the resubmission delay (default 90) →
 * save 45 → success feedback → reload shows 45 → restore 90 (the
 * settings row is stack-global).
 *
 * Journey 2: loginAs(admin) → scan an ISBN in /catalog → Admin → API
 * keys → two `propose` keys → each agent files a metadata proposal on
 * the title → `list_my_proposals` returns the agent's own proposal only,
 * with its subject, filtered by status → both keys revoked.
 *
 * The refusal inside the delay needs a rejection, which has no web
 * surface before story 11-16: it is covered over the real router by
 * `tests/proposal_history.rs` (simulated clock).
 *
 * Spec ID "PH" for ISBNs; unique key labels; creates no series, so it
 * does not depend on file order (#528).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";

const SOURCE = "https://example.org/historique";
const SAVED =
  /Proposal settings saved|Réglages des propositions enregistrés|Vorschlagseinstellungen gespeichert|Impostazioni delle proposte salvate/i;
const DELAY_INPUT = 'form#admin-system-proposals-form input[name="resubmission_delay_days"]';
const DELAY_SAVE = 'form#admin-system-proposals-form button[type="submit"]';

test.describe("Story 11-15 — proposal history and resubmission delay", () => {
  test("an admin sets the resubmission delay in Admin → System", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    await expect(
      page.getByRole("heading", { name: /^(Proposals|Propositions|Vorschläge|Proposte)$/, level: 3 }),
    ).toBeVisible();
    const input = page.locator(DELAY_INPUT);
    await expect(input).toHaveValue("90");
    await expect(input).toHaveAttribute("min", "0");
    await expect(input).toHaveAttribute("max", "3650");

    try {
      await input.fill("45");
      await page.locator(DELAY_SAVE).click();
      await expect(page.locator("#feedback-list").getByText(SAVED)).toBeVisible({
        timeout: 10000,
      });

      await page.goto("/admin?tab=system");
      await expect(page.locator(DELAY_INPUT)).toHaveValue("45");
    } finally {
      // Restore the seeded default even when an assertion failed (the
      // settings row is stack-global).
      await page.goto("/admin?tab=system");
      await page.locator(DELAY_INPUT).fill("90");
      await page.locator(DELAY_SAVE).click();
      await expect(page.locator("#feedback-list").getByText(SAVED).last()).toBeVisible({
        timeout: 10000,
      });
    }
    await page.goto("/admin?tab=system");
    await expect(page.locator(DELAY_INPUT)).toHaveValue("90");
  });

  test("an agent lists its own proposals and only its own", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(specIsbn("PH", 1));
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);

    const keyA = await createApiKey(page, "propose", "history-a");
    const keyB = await createApiKey(page, "propose", "history-b");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      // Wait for the background metadata fetch (it bumps the version).
      let version = 0;
      await expect
        .poll(
          async () => {
            const t = (await mcpCallTool(agent, keyA.plaintext, "get_title", { id: titleId }))
              .structuredContent;
            version = t.version;
            return t.publisher;
          },
          { timeout: 15000 },
        )
        .toBe("Test Publisher");

      const file = async (key: string, subtitle: string) => {
        const res = await mcpCallTool(agent, key, "propose_title_metadata", {
          title_id: titleId,
          version,
          fields: { subtitle },
          sources: [SOURCE],
        });
        expect(res.isError, JSON.stringify(res.content)).toBe(false);
        return res.structuredContent.proposal_id as number;
      };
      const mine = await file(keyA.plaintext, "Histoire A");
      const theirs = await file(keyB.plaintext, "Histoire B");

      const list = await mcpCallTool(agent, keyA.plaintext, "list_my_proposals", {
        status: "pending",
        kind: "metadata",
      });
      expect(list.isError).toBe(false);
      const items = list.structuredContent.items as Array<Record<string, unknown>>;
      const ids = items.map((i) => i.proposal_id);
      expect(ids).toContain(mine);
      expect(ids).not.toContain(theirs);
      const entry = items.find((i) => i.proposal_id === mine)!;
      expect(entry.status).toBe("pending");
      expect(entry.decided_at).toBeNull();
      expect(entry.rejection_reason).toBeNull();
      expect(entry.subject).toEqual({ title_id: titleId, fields: ["subtitle"] });
      expect(String(entry.filed_at)).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);

      // Nothing of this key was rejected.
      const rejected = await mcpCallTool(agent, keyA.plaintext, "list_my_proposals", {
        status: "rejected",
      });
      expect(rejected.structuredContent.total_items).toBe(0);

      // An unknown status is an argument error naming the valid ones.
      const bad = await mcpCallTool(agent, keyA.plaintext, "list_my_proposals", {
        status: "refused",
      });
      expect(bad.isError).toBe(true);
      expect(bad.content[0].text).toContain("pending, accepted, rejected, stale");
    } finally {
      await agent.dispose();
    }

    await page.goto("/admin?tab=api_keys");
    await revokeApiKey(page, keyA.label);
    await revokeApiKey(page, keyB.label);
  });
});
