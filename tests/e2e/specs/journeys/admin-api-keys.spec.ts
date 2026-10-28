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

const PROPOSE_LABEL = /Read \+ propose|Lecture \+ propositions|Lesen \+ vorschlagen|Lettura \+ proposte/i;

test.describe("Story 11-5 — propose API-key scope", () => {
  test("admin creates a propose key that reads but cannot write", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    await page.goto("/admin?tab=api_keys");
    const label = `agent-${crypto.randomUUID().slice(0, 8)}`;

    await page.locator("#admin-api-keys-label").fill(label);
    const proposeRadio = page.getByRole("radio", { name: PROPOSE_LABEL });
    await expect(proposeRadio).toBeVisible();
    await proposeRadio.check();
    await page
      .locator("#admin-api-keys-panel form")
      .getByRole("button", { name: /Create key|Créer la clé|Schlüssel erstellen|Crea chiave/i })
      .click();

    // One-time plaintext modal: distinct `mybibli_pr_` prefix.
    const plaintextBox = page.locator("[data-api-key-plaintext]");
    await expect(plaintextBox).toBeVisible({ timeout: 10000 });
    const plaintext = (await plaintextBox.textContent())?.trim() ?? "";
    expect(plaintext).toMatch(/^mybibli_pr_[A-Za-z0-9]{40}$/);
    await page.locator('#admin-modal-slot [data-action="admin-modal-close"]').click();
    await expect(plaintextBox).toHaveCount(0);

    // The list row carries the propose chip and the 12-char prefix.
    const row = page.locator("#admin-api-keys-list tr", { hasText: label });
    await expect(row).toBeVisible();
    await expect(row).toContainText(PROPOSE_LABEL);
    await expect(row).toContainText(plaintext.slice(0, 12));

    // The key reads the API…
    const auth = { Authorization: `Bearer ${plaintext}` };
    const read = await page.request.get("/api/v1/genres", { headers: auth });
    expect(read.status()).toBe(200);

    // …but never writes the catalog directly (NFR44).
    const patch = await page.request.patch("/api/v1/titles/1", {
      headers: { ...auth, "Content-Type": "application/json" },
      data: { version: 0, subtitle: "agent was here" },
    });
    expect(patch.status()).toBe(403);
    const body = await patch.json();
    expect(body.reason).toBe("insufficient_scope");
    expect(body.scope_required).toBe("write");

    // Clean up: revoke through the UX-DR8 modal.
    await row.getByRole("button", { name: new RegExp(label) }).click();
    const revokeDialog = page.locator("#admin-modal-slot dialog");
    await expect(revokeDialog).toBeVisible();
    await revokeDialog
      .getByRole("button", { name: /Revoke key|Révoquer la clé|Schlüssel widerrufen|Revoca chiave/i })
      .click();
    await expect(
      page.locator("#admin-api-keys-list tr", { hasText: label }),
    ).toContainText(/Revoked|Révoquée|Widerrufen|Revocata/i);

    // A revoked propose key no longer authenticates.
    const afterRevoke = await page.request.get("/api/v1/genres", { headers: auth });
    expect(afterRevoke.status()).toBe(401);
  });
});
