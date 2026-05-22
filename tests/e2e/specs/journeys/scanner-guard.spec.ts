/**
 * Story 7-5 — scanner-guard.js modal keystroke capture.
 *
 * Two test groups:
 *   1. Unit tests (about:blank + addScriptTag) — exercise the guard's
 *      policy in isolation from the app.
 *   2. Smoke test — authenticated /catalog session, inject a test
 *      <dialog> via page.evaluate, prove burst containment and normal
 *      flow resumption on close.
 *
 * Spec ID: "SG" (unique per spec, per helpers/isbn.ts convention).
 *
 * No production DOM modal exists today. Validation via a test-only
 * `<dialog>` fixture is the AC-blessed approach (Story 7-5 §Test fixture
 * approach) — the guard ships as a latent safety net for UX-DR8 modals
 * arriving in Epic 9.
 */
import { test, expect, Page } from "@playwright/test";
import path from "path";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { simulateScan } from "../../helpers/scanner";

const SCRIPT_PATH = path.resolve(
  __dirname,
  "../../../../static/js/scanner-guard.js",
);

/**
 * Set the test-hooks flag, then load the guard. Order matters — the guard
 * reads `window.__MYBIBLI_TEST_HOOKS` at IIFE start, so the flag must be
 * in place before the script tag is injected.
 */
async function loadGuard(page: Page): Promise<void> {
  await page.goto("about:blank");
  await page.addScriptTag({
    content: "window.__MYBIBLI_TEST_HOOKS = true;",
  });
  await page.addScriptTag({ path: SCRIPT_PATH });
  await page.waitForFunction(
    () => typeof (window as unknown as Record<string, unknown>).mybibliScannerGuard === "object",
  );
}

test.describe("scanner-guard — unit", () => {
  test("Test 1: stack depth tracks dialog[open] lifecycle", async ({ page }) => {
    await loadGuard(page);

    expect(
      await page.evaluate(() => window.mybibliScannerGuard.getStackDepth()),
    ).toBe(0);

    await page.evaluate(() => {
      const d = document.createElement("dialog");
      d.setAttribute("open", "");
      d.id = "m1";
      document.body.appendChild(d);
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });
    expect(
      await page.evaluate(() => window.mybibliScannerGuard.getStackDepth()),
    ).toBe(1);
    expect(
      await page.evaluate(() => window.mybibliScannerGuard.isActive()),
    ).toBe(true);

    await page.evaluate(() => {
      document.getElementById("m1")!.remove();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });
    expect(
      await page.evaluate(() => window.mybibliScannerGuard.getStackDepth()),
    ).toBe(0);
    expect(
      await page.evaluate(() => window.mybibliScannerGuard.isActive()),
    ).toBe(false);
  });

  test("Test 2: scan burst routes to focused text input inside modal", async ({
    page,
  }) => {
    await loadGuard(page);

    // Background scan-field sibling — proves the burst did not reach it.
    await page.evaluate(() => {
      document.body.insertAdjacentHTML(
        "beforeend",
        '<input id="scan-field" type="text" />' +
          '<dialog id="m2" open><input id="m2-input" type="text" /></dialog>',
      );
      document.getElementById("m2-input")!.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });

    await page.keyboard.type("9782070360246", { delay: 20 });

    expect(await page.locator("#m2-input").inputValue()).toBe("9782070360246");
    expect(await page.locator("#scan-field").inputValue()).toBe("");
  });

  test("Test 3: burst with focus on #scan-field (outside modal) does NOT leak while modal open", async ({
    page,
  }) => {
    // This is the true drop-path test: the focused element is OUTSIDE the
    // modal, the guard captures in document-capture phase, preventDefaults
    // and stopPropagations so #scan-field never receives the burst. If the
    // guard were disabled, keystrokes would land naturally into the focused
    // #scan-field (because that IS where the browser sends them).
    await loadGuard(page);

    let scanFieldInputEvents = 0;
    await page.exposeFunction("__countScanFieldInput", () => {
      scanFieldInputEvents++;
    });

    await page.evaluate(() => {
      document.body.insertAdjacentHTML(
        "beforeend",
        '<input id="scan-field" type="text" />' +
          '<dialog id="m3" open><button id="c3" type="button">Cancel</button></dialog>',
      );
      document
        .getElementById("scan-field")!
        .addEventListener("input", () =>
          (window as unknown as { __countScanFieldInput: () => void }).__countScanFieldInput(),
        );
      // Focus #scan-field INTENTIONALLY to prove the guard protects it
      // even though it is where the browser would naturally deliver keys.
      document.getElementById("scan-field")!.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });

    await page.keyboard.type("9782070360246", { delay: 20 });

    expect(await page.locator("#scan-field").inputValue()).toBe("");
    expect(scanFieldInputEvents).toBe(0);
    expect(
      await page.evaluate(() => document.getElementById("m3")!.hasAttribute("open")),
    ).toBe(true);
  });

  test("Test 4: LIFO nesting routes bursts to the top-of-stack modal", async ({
    page,
  }) => {
    await loadGuard(page);

    await page.evaluate(() => {
      document.body.insertAdjacentHTML(
        "beforeend",
        '<input id="scan-field" type="text" />' +
          '<div id="ma" aria-modal="true">' +
          '<input id="ma-input" type="text" />' +
          '<div id="mb" aria-modal="true">' +
          '<input id="mb-input" type="text" />' +
          "</div></div>",
      );
      document.getElementById("mb-input")!.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });

    // Top of stack is mb (document order — mb is after ma). Focused
    // mb-input is inside top modal → pass-through → value populates.
    await page.keyboard.type("111", { delay: 20 });
    expect(await page.locator("#mb-input").inputValue()).toBe("111");

    // Close mb, focus ma-input, type again.
    await page.evaluate(() => {
      document.getElementById("mb")!.remove();
      document.getElementById("ma-input")!.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });
    await page.keyboard.type("222", { delay: 20 });
    expect(await page.locator("#ma-input").inputValue()).toBe("222");

    // Close ma, focus #scan-field, type — no modal, listener absent.
    await page.evaluate(() => {
      document.getElementById("ma")!.remove();
      document.getElementById("scan-field")!.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });
    await page.keyboard.type("333", { delay: 20 });
    expect(await page.locator("#scan-field").inputValue()).toBe("333");
  });

  test("Test 5: no modal = pass-through, guard installs no keydown listener", async ({
    page,
  }) => {
    await loadGuard(page);

    await page.evaluate(() => {
      document.body.insertAdjacentHTML(
        "beforeend",
        '<input id="scan-field" type="text" />',
      );
      document.getElementById("scan-field")!.focus();
    });

    await page.keyboard.type("ABC", { delay: 20 });

    expect(await page.locator("#scan-field").inputValue()).toBe("ABC");
    expect(
      await page.evaluate(() => window.mybibliScannerGuard.getStackDepth()),
    ).toBe(0);
    // Guard's capture-phase listener was never installed (no open→close
    // transition occurred), so the attach counter stays at 0.
    expect(
      await page.evaluate(() =>
        window.__mybibliScannerGuardTestHooks.listenerAttachCount(),
      ),
    ).toBe(0);
  });

  test("Test 6: idempotent load — sentinel blocks repeat IIFE execution", async ({
    page,
  }) => {
    await loadGuard(page);

    // Load the script 4 extra times. Each IIFE should early-return via the
    // `window.__mybibliScannerGuardWired` sentinel. If it didn't, opening
    // a modal would install the listener 5 times and we would observe
    // attachCount=5 instead of 1.
    for (let i = 0; i < 4; i++) {
      await page.addScriptTag({ path: SCRIPT_PATH });
    }

    expect(
      await page.evaluate(() => window.__mybibliScannerGuardWired),
    ).toBe(true);

    await page.evaluate(() => {
      const d = document.createElement("dialog");
      d.setAttribute("open", "");
      d.id = "m6";
      document.body.appendChild(d);
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });

    expect(
      await page.evaluate(() =>
        window.__mybibliScannerGuardTestHooks.listenerAttachCount(),
      ),
    ).toBe(1);

    await page.evaluate(() => {
      document.getElementById("m6")!.remove();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });

    expect(
      await page.evaluate(() =>
        window.__mybibliScannerGuardTestHooks.listenerDetachCount(),
      ),
    ).toBe(1);
  });

  test("Test 7: MutationObserver engages/disengages on attribute flip", async ({
    page,
  }) => {
    await loadGuard(page);

    await page.evaluate(() => {
      const d = document.createElement("dialog");
      d.id = "m7";
      document.body.appendChild(d);
    });
    // No `open` attribute yet — guard stack stays empty.
    await expect
      .poll(() => page.evaluate(() => window.mybibliScannerGuard.getStackDepth()))
      .toBe(0);

    await page.evaluate(() => {
      document.getElementById("m7")!.setAttribute("open", "");
    });
    await expect
      .poll(() => page.evaluate(() => window.mybibliScannerGuard.getStackDepth()))
      .toBe(1);

    await page.evaluate(() => {
      document.getElementById("m7")!.removeAttribute("open");
    });
    await expect
      .poll(() => page.evaluate(() => window.mybibliScannerGuard.getStackDepth()))
      .toBe(0);
  });

  test("Test 8: Enter on focused button inside modal does NOT activate", async ({
    page,
  }) => {
    await loadGuard(page);

    await page.evaluate(() => {
      document.body.insertAdjacentHTML(
        "beforeend",
        '<input id="scan-field" type="text" />' +
          '<dialog id="m8" open>' +
          '<button id="c8" type="button">Cancel</button>' +
          "</dialog>",
      );
      const btn = document.getElementById("c8")!;
      (window as unknown as Record<string, unknown>).__clickedCount = 0;
      btn.addEventListener("click", () => {
        ((window as unknown as Record<string, number>).__clickedCount as number)++;
      });
      btn.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });

    await page.keyboard.type("9782070360246", { delay: 20 });
    await page.keyboard.press("Enter");

    const clicked = await page.evaluate(
      () => (window as unknown as Record<string, number>).__clickedCount,
    );
    expect(clicked).toBe(0);
    expect(
      await page.evaluate(() =>
        document.getElementById("m8")!.hasAttribute("open"),
      ),
    ).toBe(true);
    expect(await page.locator("#scan-field").inputValue()).toBe("");
  });

  // #31 sub-item 1 — maxLength honored.
  test("Test 9: scan burst respects maxLength on the focused modal input", async ({
    page,
  }) => {
    await loadGuard(page);

    await page.evaluate(() => {
      document.body.insertAdjacentHTML(
        "beforeend",
        '<input id="scan-field" type="text" />' +
          '<dialog id="m9" open>' +
          '<input id="m9-input" type="text" maxlength="5" />' +
          "</dialog>",
      );
      document.getElementById("m9-input")!.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();
    });

    // 10-char ISBN prefix burst into an input capped at maxlength=5 —
    // the guard must stop forwarding once the limit is reached. Before
    // #31's fix, .value was appended unbounded, so all 10 chars landed.
    await page.keyboard.type("9782070360", { delay: 20 });

    expect(await page.locator("#m9-input").inputValue()).toBe("97820");
    expect(
      await page.locator("#m9-input").inputValue().then((v) => v.length),
    ).toBe(5);
    expect(await page.locator("#scan-field").inputValue()).toBe("");
  });

  // #31 sub-item 1 — IME composition skipped. Synthesize a keydown with
  // isComposing=true and assert no .value mutation occurs. The real-world
  // race is too narrow to provoke with Playwright's IME APIs (no first-
  // class composition simulation), so we drive the listener directly.
  test("Test 10: scan burst skipped while IME composition is active", async ({
    page,
  }) => {
    await loadGuard(page);

    const valueAfter = await page.evaluate(() => {
      document.body.insertAdjacentHTML(
        "beforeend",
        '<dialog id="m10" open>' +
          '<input id="m10-input" type="text" />' +
          "</dialog>",
      );
      const input = document.getElementById("m10-input") as HTMLInputElement;
      input.focus();
      window.__mybibliScannerGuardTestHooks.refreshStack();

      // Dispatch a keydown on document, mimicking the scanner — but with
      // isComposing=true to assert the guard early-returns. Without
      // the fix, the .value would be set to "a".
      const ev = new KeyboardEvent("keydown", {
        key: "a",
        code: "KeyA",
        bubbles: true,
        cancelable: true,
        isComposing: true,
      });
      // KeyboardEvent's isComposing is read-only in some browsers; if it
      // didn't take, fall back to defining it on the instance for the
      // duration of dispatch.
      if (!ev.isComposing) {
        Object.defineProperty(ev, "isComposing", { value: true });
      }
      document.dispatchEvent(ev);
      return input.value;
    });

    expect(valueAfter).toBe("");
  });
});

test.describe("scanner-guard — E2E", () => {
  test("scanner burst is contained by open dialog; normal flow resumes on close", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");
    await expect(page.locator("#scan-field")).toBeVisible();

    // Count requests to /catalog/scan — must stay at 0 during the guarded burst.
    let scanRequestCount = 0;
    page.on("request", (req) => {
      try {
        const url = new URL(req.url());
        if (url.pathname === "/catalog/scan") scanRequestCount++;
      } catch {
        // Ignore non-URL request values (e.g., data: URLs in tests).
      }
    });

    // Act 1 — inject test dialog via showModal().
    await page.evaluate(() => {
      const d = document.createElement("dialog");
      d.id = "test-guard-modal";
      d.innerHTML =
        '<input id="test-modal-input" type="text" />' +
        '<button id="test-modal-cancel" type="button">Cancel</button>';
      document.body.appendChild(d);
      (d as HTMLDialogElement).showModal();
      document.getElementById("test-modal-input")!.focus();
    });
    await expect
      .poll(() => page.evaluate(() => window.mybibliScannerGuard.getStackDepth()))
      .toBeGreaterThanOrEqual(1);

    // Act 2 — burst into the modal input via the helper.
    const scanA = specIsbn("SG", 1);
    await simulateScan(page, "#test-modal-input", scanA);
    expect(await page.locator("#test-modal-input").inputValue()).toBe(scanA);
    expect(await page.locator("#scan-field").inputValue()).toBe("");
    expect(scanRequestCount).toBe(0);

    // Act 3 — close dialog, focus #scan-field, burst into the normal flow.
    await page.evaluate(() => {
      const d = document.getElementById("test-guard-modal") as HTMLDialogElement | null;
      if (d) d.close();
      document.getElementById("scan-field")!.focus();
    });
    await expect
      .poll(() => page.evaluate(() => window.mybibliScannerGuard.getStackDepth()))
      .toBe(0);

    const scanB = specIsbn("SG", 2);
    await simulateScan(page, "#scan-field", scanB);

    // A feedback entry must appear for the normal scan flow.
    await expect(
      page.locator("#feedback-list .feedback-entry, #feedback-list .feedback-skeleton").first(),
    ).toBeVisible({ timeout: 10000 });
    expect(scanRequestCount).toBeGreaterThanOrEqual(1);

    // Teardown — remove injected dialog so parallel specs see a clean DOM.
    await page.evaluate(() => {
      const d = document.getElementById("test-guard-modal");
      if (d) d.remove();
    });
  });
});

declare global {
  interface Window {
    mybibliScannerGuard: {
      getStackDepth(): number;
      isActive(): boolean;
    };
    __mybibliScannerGuardTestHooks: {
      listenerAttachCount(): number;
      listenerDetachCount(): number;
      refreshStack(): void;
    };
    __mybibliScannerGuardWired?: boolean;
    __MYBIBLI_TEST_HOOKS?: boolean;
  }
}
