import { expect, type Locator } from "@playwright/test";

/**
 * Story 19-2 (#685) — wait until HTMX has wired a node it just swapped in.
 *
 * htmx 2 inserts a swapped node with the `htmx-added` class and processes it
 * (`hx-post`, triggers, …) in a settle task after `defaultSettleDelay`
 * (20 ms, a `setTimeout` that a loaded runner can delay). The class is
 * removed in the same task, so once it is gone the node is live. Typing
 * Enter into, or clicking the submit of, a form swapped in moments ago can
 * otherwise reach a form HTMX has not wired yet: a form with only `hx-post`
 * (no `method` / `action`) then does a native GET submit and the page
 * reloads.
 *
 * `locator` must be a top-level node of the swapped fragment (the element
 * that carries `htmx-added`), e.g. `#title-edit-form`.
 */
export async function expectHtmxProcessed(locator: Locator, timeout = 10000): Promise<void> {
  await expect(locator).toBeVisible({ timeout });
  await expect(locator).not.toHaveClass(/(^|\s)htmx-added(\s|$)/, { timeout });
}
