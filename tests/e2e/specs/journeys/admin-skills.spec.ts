import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";

/**
 * Story 15-2 (#614, FR162, UX-DR40) — Admin → Skills.
 *
 * Journey from a blank browser: an admin opens the Skills tab, creates a
 * skill whose body carries HTML (shown as text, never rendered), edits
 * it, deactivates it, deletes it through the UX-DR8 modal, finds it in
 * the Trash and restores it. A librarian is refused.
 *
 * specId "SK": every skill name is `sk-<random>`, so parallel workers and
 * repeated runs never collide on the shared database (no ISBN needed).
 */

const SKILLS_TAB = /^(Skills|Compétences|Skill)$/i;
const ACTIVE = /^(Active|Aktiv|Attivo)$/i;
const INACTIVE = /^(Inactive|Inaktiv|Inattivo)$/i;

function skillName(): string {
  return `sk-${crypto.randomUUID().slice(0, 8)}`;
}

async function openSkillsTab(page: Page) {
  await page.goto("/admin", { waitUntil: "domcontentloaded" });
  await page.getByRole("tab", { name: SKILLS_TAB }).click();
  await expect(page.locator("#admin-skills-editor")).toBeVisible({ timeout: 10000 });
  await expect(page).toHaveURL(/\/admin\?tab=skills/);
}

const rowOf = (page: Page, name: string) =>
  page.locator("#admin-skills-list tbody tr").filter({
    has: page.getByText(name, { exact: true }),
  });

test.describe("Story 15-2 — MCP skills admin tab", () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
  });

  test("admin creates, edits, deactivates, deletes and restores a skill", async ({ page }) => {
    await loginAs(page, "admin");
    await openSkillsTab(page);
    await checkAccessibility(page);

    const name = skillName();
    const body = `# Catalog rules\n\n<img src=x data-probe="${name}">\n**Bold** stays Markdown.`;
    const editor = page.locator("#admin-skills-editor");
    await editor.locator("#admin-skills-name").fill(name);
    await editor.locator("#admin-skills-description").fill("How the agent catalogs comics");
    await editor.locator("#admin-skills-body").fill(body);
    await editor.locator("#admin-skills-submit").click();

    const row = rowOf(page, name);
    await expect(row).toBeVisible({ timeout: 10000 });
    await expect(row.getByText(ACTIVE)).toBeVisible();
    // The editor is back to a blank create form, focus kept on its submit
    // button (Rule #21: restored, not redirected).
    await expect(editor.locator("#admin-skills-name")).toHaveValue("");
    await expect(page.locator("#admin-skills-submit")).toBeFocused();

    // AC4 — the body is text: the <img> markup is displayed, never an element.
    await row.locator("summary").click();
    await expect(row.locator("pre")).toContainText(`<img src=x data-probe="${name}">`);
    await expect(row.locator("pre")).toContainText("**Bold** stays Markdown.");
    await expect(page.locator(`img[data-probe="${name}"]`)).toHaveCount(0);

    // Edit: the editor fills, the save updates the row.
    await row.locator('button[id^="admin-skill-edit-"]').click();
    await expect(editor.locator("#admin-skills-name")).toHaveValue(name, { timeout: 10000 });
    await expect(editor.locator("#admin-skills-body")).toHaveValue(body);
    await checkAccessibility(page);
    await editor.locator("#admin-skills-description").fill("Comics, revised");
    await editor.locator("#admin-skills-submit").click();
    await expect(rowOf(page, name)).toContainText("Comics, revised", { timeout: 10000 });
    await expect(editor.locator("#admin-skills-name")).toHaveValue("");

    // Deactivate: the chip flips, focus stays on the row's toggle button.
    const toggle = rowOf(page, name).locator('button[id^="admin-skill-toggle-"]');
    const toggleId = await toggle.getAttribute("id");
    await toggle.click();
    await expect(rowOf(page, name).getByText(INACTIVE)).toBeVisible({ timeout: 10000 });
    await expect(page.locator(`#${toggleId}`)).toBeFocused();

    // Delete through the UX-DR8 modal → the skill leaves the list.
    await rowOf(page, name).locator('button[id^="admin-skill-delete-"]').click();
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });
    await modal.locator("[data-modal-confirm]").click();
    await expect(modal).toHaveCount(0, { timeout: 10000 });
    await expect(rowOf(page, name)).toHaveCount(0, { timeout: 10000 });

    // Its name stays reserved while it sits in the Trash: a new skill with
    // that name is refused, the trashed one is not overwritten (review M1).
    await editor.locator("#admin-skills-name").fill(name);
    await editor.locator("#admin-skills-description").fill("Should not replace it");
    await editor.locator("#admin-skills-body").fill("replacement text");
    await editor.locator("#admin-skills-submit").click();
    await expect(page.locator(".feedback-entry").last()).toContainText(
      /Trash|corbeille|Papierkorb|cestino/i,
      { timeout: 10000 },
    );
    await expect(editor.locator("#admin-skills-name")).toHaveValue(name);
    await expect(rowOf(page, name)).toHaveCount(0);

    // The Trash lists it as a skill; restore brings it back, still inactive.
    await page.goto("/admin?tab=trash", { waitUntil: "domcontentloaded" });
    await page.locator("#filter-entity-type").selectOption("mcp_skills");
    const trashRow = page.locator("#admin-trash-panel tbody tr").filter({ hasText: name });
    await expect(trashRow).toBeVisible({ timeout: 10000 });
    await trashRow
      .getByRole("button", { name: /^(restore|restaurer|wiederherstellen|ripristina)$/i })
      .click();
    await expect(page.locator("#admin-trash-panel tbody tr").filter({ hasText: name })).toHaveCount(
      0,
      { timeout: 10000 },
    );

    await openSkillsTab(page);
    await expect(rowOf(page, name)).toBeVisible();
    await expect(rowOf(page, name).getByText(INACTIVE)).toBeVisible();
    // Restored with its own text, not the refused replacement.
    await rowOf(page, name).locator("summary").click();
    await expect(rowOf(page, name).locator("pre")).toContainText("**Bold** stays Markdown.");
  });

  test("a malformed name is refused with a field hint", async ({ page }) => {
    await loginAs(page, "admin");
    await openSkillsTab(page);
    const editor = page.locator("#admin-skills-editor");
    await editor.locator("#admin-skills-name").fill("Not A Slug");
    await editor.locator("#admin-skills-description").fill("whatever");
    await editor.locator("#admin-skills-body").fill("text");
    await editor.locator("#admin-skills-submit").click();
    await expect(page.locator(".feedback-entry").last()).toContainText(
      /lowercase|minuscules|Kleinbuchstaben|minuscole/i,
      { timeout: 10000 },
    );
    // What the admin typed is still there to fix.
    await expect(editor.locator("#admin-skills-name")).toHaveValue("Not A Slug");
  });

  test("a librarian cannot reach the Skills tab", async ({ page }) => {
    await loginAs(page, "librarian");
    for (const path of ["/admin/skills", "/admin?tab=skills"]) {
      const resp = await page.request.get(path, { failOnStatusCode: false, maxRedirects: 0 });
      expect(resp.status(), path).toBe(403);
    }
    // A POST carrying a valid CSRF token reaches the handler's role check.
    await page.goto("/", { waitUntil: "domcontentloaded" });
    const csrf = await page.locator('meta[name="csrf-token"]').getAttribute("content");
    const post = await page.request.post("/admin/skills", {
      failOnStatusCode: false,
      maxRedirects: 0,
      headers: { "X-CSRF-Token": csrf ?? "" },
      form: { name: skillName(), description: "d", body: "b", active: "1" },
    });
    expect(post.status()).toBe(403);
  });
});
