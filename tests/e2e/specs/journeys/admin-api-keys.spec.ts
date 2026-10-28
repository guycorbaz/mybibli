/**
 * Story 11-5 E2E — the `propose` API-key scope (FR126, NFR44).
 *
 * Real journey from a blank browser: loginAs(admin) → Admin → API keys
 * → create a key with the `propose` scope → the one-time modal shows a
 * `mybibli_pr_…` plaintext → the list shows the key with its scope chip
 * → the key reads /api/v1 (200) but cannot PATCH a title (403
 * insufficient_scope) → revoke it so no active key is left behind.
 *
 * Per-test unique label; no shared-DB assertion beyond the row this
 * test creates.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { createApiKey, revokeApiKey, SCOPE_LABEL } from "../../helpers/api-keys";

const PROPOSE_LABEL = SCOPE_LABEL.propose;

test.describe("Story 11-5 — propose API-key scope", () => {
  test("admin creates a propose key that reads but cannot write", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const { label, plaintext } = await createApiKey(page, "propose");
    // One-time plaintext modal: distinct `mybibli_pr_` prefix.
    expect(plaintext).toMatch(/^mybibli_pr_[A-Za-z0-9]{40}$/);

    // The list row carries the propose chip and the 12-char prefix.
    const row = page.locator("#admin-api-keys-list tr", { hasText: label });
    await expect(row).toBeVisible();
    await expect(row).toContainText(PROPOSE_LABEL);
    await expect(row).toContainText(plaintext.slice(0, 12));

    // The key reads the API…
    const auth = { Authorization: `Bearer ${plaintext}` };
    for (const path of ["/api/v1/titles", "/api/v1/genres"]) {
      const read = await page.request.get(path, { headers: auth });
      expect(read.status(), path).toBe(200);
    }

    // …but never writes the catalog directly (NFR44). The id does not
    // exist on purpose: the scope check runs before the title lookup, so
    // the expected answer is 403; a regressed guard would answer 404 and
    // still fail this test without touching any shared row.
    const patch = await page.request.patch("/api/v1/titles/2147483647", {
      headers: { ...auth, "Content-Type": "application/json" },
      data: { version: 0, subtitle: "agent was here" },
    });
    expect(patch.status()).toBe(403);
    const body = await patch.json();
    expect(body.reason).toBe("insufficient_scope");
    expect(body.scope_required).toBe("write");

    // Clean up: revoke through the UX-DR8 modal.
    await revokeApiKey(page, label);
    // The revoked row loses its Revoke button (the status chip and the
    // German Revoke button share the word "Widerrufen", so assert on the
    // button, not on the row text).
    const revokedRow = page.locator("#admin-api-keys-list tr", { hasText: label });
    await expect(revokedRow).toBeVisible();
    await expect(
      revokedRow.getByRole("button", {
        name: /Delete API key|Supprimer la clé|löschen|Elimina chiave/i,
      }),
    ).toBeVisible();
    await expect(
      revokedRow.getByRole("button", { name: /Revoke API key|Révoquer la clé|widerrufen|Revoca chiave/i }),
    ).toHaveCount(0);

    // A revoked propose key no longer authenticates.
    const afterRevoke = await page.request.get("/api/v1/genres", { headers: auth });
    expect(afterRevoke.status()).toBe(401);
  });
});
