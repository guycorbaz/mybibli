import { Locator, expect } from "@playwright/test";

/**
 * Story 19-2 (D9, retro v1.28.1 A1) — "really seen" assertions.
 *
 * Playwright's `toBeVisible()` only asks for a non-empty box and no
 * `visibility: hidden`: an element at `opacity: 0` — or inside an ancestor
 * at `opacity: 0` — passes. #695 shipped that way (tree row controls at
 * opacity 0 in the Docker image). These helpers read what a person would
 * actually see:
 *
 * - `seenState(locator)` — the element's effective opacity (the product of
 *   its own and every ancestor's computed opacity), its computed
 *   `visibility`, the first element up the tree with `display: none`
 *   (`hiddenBy`), its box size and whether the box meets the viewport.
 * - `effectiveOpacity(locator)` — the effective opacity alone, for
 *   `expect.poll(() => effectiveOpacity(x)).toBe(1)` after a hover / focus.
 * - `expectReallySeen(locator, { minOpacity = 0.5, inViewport = false,
 *   timeout })` — polls until the element is attached, displayed, not
 *   `visibility: hidden`, has a non-empty box, an effective opacity of at
 *   least `minOpacity` and (optionally) a box meeting the viewport. On
 *   failure the received value lists every problem with the full state.
 *
 * The locator must resolve to a single element (strict, like any
 * Playwright assertion).
 */

export interface SeenState {
  /** False when the locator matches nothing. */
  attached: boolean;
  /** Product of the computed opacity of the element and all its ancestors. */
  opacity: number;
  /** The element's computed `visibility` (inherited from its ancestors). */
  visibility: string;
  /** The first element (self or ancestor) with `display: none`, or null. */
  hiddenBy: string | null;
  width: number;
  height: number;
  /** The box meets the viewport (at least one pixel in each direction). */
  inViewport: boolean;
}

const DETACHED: SeenState = {
  attached: false,
  opacity: 0,
  visibility: "n/a",
  hiddenBy: null,
  width: 0,
  height: 0,
  inViewport: false,
};

/** One reading of what a person would see of `locator` right now. */
export async function seenState(locator: Locator): Promise<SeenState> {
  const matches = await locator.count();
  if (matches === 0) return DETACHED;
  if (matches > 1) {
    // Report the real cause instead of polling a strict-mode error as "detached".
    throw new Error(`seenState: the locator matches ${matches} elements — narrow it to one`);
  }
  try {
    return await locator.evaluate(
      (el) => {
        const describe = (node: Element): string => {
          const id = node.id ? `#${node.id}` : "";
          const cls =
            typeof node.className === "string" && node.className.trim() !== ""
              ? "." + node.className.trim().split(/\s+/).slice(0, 3).join(".")
              : "";
          return `${node.tagName.toLowerCase()}${id}${cls}`;
        };
        let opacity = 1;
        let hiddenBy: string | null = null;
        for (let node: Element | null = el; node; node = node.parentElement) {
          const style = getComputedStyle(node);
          opacity *= Number(style.opacity);
          if (hiddenBy === null && style.display === "none") hiddenBy = describe(node);
        }
        const rect = el.getBoundingClientRect();
        const inViewport =
          rect.width > 0 &&
          rect.height > 0 &&
          rect.right > 0 &&
          rect.bottom > 0 &&
          rect.left < window.innerWidth &&
          rect.top < window.innerHeight;
        return {
          attached: true,
          opacity,
          visibility: getComputedStyle(el).visibility,
          hiddenBy,
          width: Math.round(rect.width),
          height: Math.round(rect.height),
          inViewport,
        };
      },
      undefined,
      { timeout: 2000 },
    );
  } catch {
    // Detached between count() and evaluate() (an HTMX swap): read again
    // on the next poll.
    return DETACHED;
  }
}

/** The element's effective opacity (itself times every ancestor). */
export async function effectiveOpacity(locator: Locator): Promise<number> {
  return (await seenState(locator)).opacity;
}

export interface ReallySeenOptions {
  /** Lowest effective opacity that counts as seen (default 0.5). */
  minOpacity?: number;
  /** Also require the box to meet the viewport (default false). */
  inViewport?: boolean;
  /** Poll timeout in ms (default: the project's expect timeout). */
  timeout?: number;
}

/** What keeps `state` from being seen; empty when it is seen. */
function problems(state: SeenState, minOpacity: number, inViewport: boolean): string[] {
  if (!state.attached) return ["not attached"];
  const found: string[] = [];
  if (state.hiddenBy !== null) found.push(`display:none on ${state.hiddenBy}`);
  if (state.visibility !== "visible") found.push(`visibility ${state.visibility}`);
  if (state.width === 0 || state.height === 0) found.push(`empty box ${state.width}x${state.height}`);
  if (state.opacity < minOpacity) {
    found.push(`effective opacity ${state.opacity.toFixed(2)} < ${minOpacity}`);
  }
  if (inViewport && !state.inViewport) found.push("outside the viewport");
  if (found.length > 0) found.push(`state ${JSON.stringify(state)}`);
  return found;
}

/**
 * Waits until `locator` is really seen (see the module doc). Fails with
 * the list of problems and the last state read.
 */
export async function expectReallySeen(
  locator: Locator,
  { minOpacity = 0.5, inViewport = false, timeout }: ReallySeenOptions = {},
): Promise<void> {
  await expect
    .poll(async () => problems(await seenState(locator), minOpacity, inViewport), {
      message: `${locator} should be really seen (opacity >= ${minOpacity}${
        inViewport ? ", in the viewport" : ""
      })`,
      ...(timeout === undefined ? {} : { timeout }),
    })
    .toEqual([]);
}
