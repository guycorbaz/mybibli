/**
 * Story 13-9 E2E (#581, FR156) — an agent proposes a label on a doubtful
 * volume; the librarian accepts it.
 *
 * Real journey from a blank browser: loginAs(admin) → create the label
 * "Code douteux …" in Admin → Reference data → catalogue a title and a
 * volume by scan → create a `propose` key → as a separate agent client:
 * `tools/list` lists `list_labels` and `propose_label` → `list_labels`
 * gives the label id → `get_title` gives the volume id (no label yet) →
 * `propose_label` files a pending `label_attach` proposal → the same
 * proposal again is refused → loginAs(librarian) → the proposal page
 * shows the volume, the label and the note → accept through the UX-DR8
 * modal → the volume page carries the label → `/labels/{id}` lists the
 * volume → `get_title` reports it → the key is revoked.
 *
 * Spec ID "LB" for ISBNs (random sequence, so a rerun against an
 * un-reset DB starts from a fresh title); V-code band V4500–V4999 (unused
 * by any other spec); unique label and key names; assertions only on rows
 * this test creates.
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { volumeIdByLabel } from "../../helpers/catalog";
import { createApiKey } from "../../helpers/api-keys";
import { createLabel } from "../../helpers/labels";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";
import { acceptFromDetail, revokeProposeKey } from "../../helpers/proposals";

const SOURCE = "https://example.org/notice-lb";
const NOTE = "The ISBN printed on this copy belongs to another book.";

test.describe("Story 13-9 — the agent proposes a label", () => {
  test.setTimeout(90000);

  test("an agent marks a doubtful volume and the librarian accepts", async ({
    page,
    baseURL,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const labelName = `Code douteux LB-${crypto.randomUUID().slice(0, 8)}`;
    await createLabel(page, labelName);

    const isbn = specIsbn("LB", 1 + Math.floor(Math.random() * 89999));
    const vcode = `V${4500 + Math.floor(Math.random() * 500)}`;
    await scanTitleAndVolume(page, isbn, vcode);
    await page.goto(`/?q=${isbn}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await expect(page).toHaveURL(/\/title\/\d+/);
    const titleId = Number(/\/title\/(\d+)/.exec(page.url())![1]);
    const volumeId = Number(await volumeIdByLabel(page, String(titleId), vcode));

    const propose = await createApiKey(page, "propose", "mcp-label-proposal");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    let proposalId = 0;
    let labelId = 0;
    try {
      const key = propose.plaintext;
      const list = await mcpPost(
        agent,
        key,
        { jsonrpc: "2.0", id: 1, method: "tools/list" },
        { "MCP-Protocol-Version": "2025-11-25" },
      );
      expect(list.status()).toBe(200);
      const tools = (await list.json()).result.tools;
      expect(tools.map((t: { name: string }) => t.name).sort()).toEqual(MCP_TOOLS);
      const tool = tools.find((t: { name: string }) => t.name === "propose_label");
      expect(tool.annotations.readOnlyHint).toBe(false);

      const labels = (await mcpCallTool(agent, key, "list_labels", {})).structuredContent.items;
      const label = labels.find((l: { name: string }) => l.name === labelName);
      expect(label, JSON.stringify(labels)).toBeTruthy();
      labelId = label.id;
      expect(label.volume_count).toBe(0);

      const before = (await mcpCallTool(agent, key, "get_title", { id: titleId }))
        .structuredContent;
      const volume = before.volumes.find((v: { label: string }) => v.label === vcode);
      expect(volume.id).toBe(volumeId);
      expect(volume.labels).toEqual([]);

      const filed = await mcpCallTool(agent, key, "propose_label", {
        target: { volume_id: volumeId },
        label_name: labelName,
        note: NOTE,
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      expect(filed.structuredContent.status).toBe("pending");
      expect(filed.structuredContent.kind).toBe("label_attach");
      proposalId = filed.structuredContent.proposal_id;

      // The same label on the same volume is already pending: refused.
      const again = await mcpCallTool(agent, key, "propose_label", {
        target: { volume_id: volumeId },
        label_id: labelId,
        sources: [SOURCE],
      });
      expect(again.isError).toBe(true);
      expect(again.content[0].text).toContain(`Proposal ${proposalId} already proposes`);
    } finally {
      await agent.dispose();
    }

    // The librarian reviews and accepts.
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto(`/proposals/${proposalId}`);
    const body = page.locator("#proposal-detail-body");
    await expect(body).toHaveAttribute("data-proposal-status", "pending");
    const line = page.locator('#proposal-items li[data-line="label"]');
    await expect(line.locator("[data-label-name]")).toHaveText(labelName);
    await expect(line.locator("[data-label-note]")).toHaveText(NOTE);
    await expect(line.locator("[data-label-target]")).toContainText(vcode);
    await expect(page.locator("#proposal-sources")).toContainText(SOURCE);
    await acceptFromDetail(page);

    // The volume carries the label; the label page lists the volume.
    await page.goto(`/volume/${volumeId}`);
    await expect(page.locator(`#entity-labels [data-label-chip="${labelName}"]`)).toBeVisible();
    await page.goto(`/labels/${labelId}`);
    await expect(
      page.locator("#label-volumes-table").getByRole("link", { name: vcode }),
    ).toBeVisible();

    // The agent reads it back, then the key is revoked.
    const reader = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      const after = (await mcpCallTool(reader, propose.plaintext, "get_title", { id: titleId }))
        .structuredContent;
      const volume = after.volumes.find((v: { id: number }) => v.id === volumeId);
      expect(volume.labels).toEqual([{ id: labelId, name: labelName }]);
    } finally {
      await reader.dispose();
    }
    await revokeProposeKey(page, propose.label);
  });
});
