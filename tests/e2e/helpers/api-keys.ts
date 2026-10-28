/**
 * Admin → API keys helpers (story 11-6, extracted from the 11-5 spec).
 *
 * `createApiKey` drives the real UI (radio by accessible name, one-time
 * plaintext modal) and returns the plaintext; `revokeApiKey` revokes a
 * key through its UX-DR8 modal so a spec leaves no active key behind.
 * The caller must be logged in as an admin.
 */
import { Page, expect } from "@playwright/test";

export type ApiKeyScope = "read" | "write" | "propose";

export const SCOPE_LABEL: Record<ApiKeyScope, RegExp> = {
  read: /Read only|Lecture seule|Nur lesen|Sola lettura/i,
  write: /Read \+ write|Lecture \+ écriture|Lesen \+ schreiben|Lettura \+ scrittura/i,
  propose: /Read \+ propose|Lecture \+ propositions|Lesen \+ vorschlagen|Lettura \+ proposte/i,
};

/** Create a key with a unique label; returns the label and the plaintext. */
export async function createApiKey(
  page: Page,
  scope: ApiKeyScope,
  labelPrefix = "agent",
): Promise<{ label: string; plaintext: string }> {
  await page.goto("/admin?tab=api_keys");
  const label = `${labelPrefix}-${crypto.randomUUID().slice(0, 8)}`;

  await page.locator("#admin-api-keys-label").fill(label);
  const radio = page.getByRole("radio", { name: SCOPE_LABEL[scope] });
  await expect(radio).toBeVisible();
  await radio.check();
  await page
    .locator("#admin-api-keys-panel form")
    .getByRole("button", { name: /Create key|Créer la clé|Schlüssel erstellen|Crea chiave/i })
    .click();

  const plaintextBox = page.locator("[data-api-key-plaintext]");
  await expect(plaintextBox).toBeVisible({ timeout: 10000 });
  const plaintext = (await plaintextBox.textContent())?.trim() ?? "";
  expect(plaintext).toMatch(/^mybibli_(ro|rw|pr)_[A-Za-z0-9]{40}$/);
  await page.locator('#admin-modal-slot [data-action="admin-modal-close"]').click();
  await expect(plaintextBox).toHaveCount(0);
  return { label, plaintext };
}

/** Revoke the key listed under `label` (Admin → API keys must be open). */
export async function revokeApiKey(page: Page, label: string): Promise<void> {
  const row = page.locator("#admin-api-keys-list tr", { hasText: label });
  await row.getByRole("button", { name: new RegExp(label) }).click();
  const revokeDialog = page.locator("#admin-modal-slot dialog");
  await expect(revokeDialog).toBeVisible();
  const revoked = page.waitForResponse(
    (r) => r.request().method() === "POST" && /\/admin\/api-keys\/\d+\/revoke$/.test(r.url()),
  );
  await revokeDialog
    .getByRole("button", { name: /Revoke key|Révoquer la clé|Schlüssel widerrufen|Revoca chiave/i })
    .click();
  expect((await revoked).status()).toBe(200);
  await expect(revokeDialog).toBeHidden();
}
