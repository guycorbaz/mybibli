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
 * htmx marks only the fragment's top-level nodes, and processes their
 * descendants in the same task — so `locator` may be any node inside the
 * swap (e.g. a form nested in a wrapper `<div>`, `#title-form-container
 * form`): the wait is over when neither it nor an ancestor still carries
 * the class. Limit: a locator that already matched before the swap passes
 * at once — target what the swap brings.
 */
export async function expectHtmxProcessed(locator: Locator, timeout = 10000): Promise<void> {
  await expect(locator).toBeVisible({ timeout });
  await expect
    .poll(() => locator.evaluate((el) => el.closest(".htmx-added") === null), {
      message: "the swapped node (or an ancestor) still carries htmx-added",
      timeout,
    })
    .toBe(true);
}
