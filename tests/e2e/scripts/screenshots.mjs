// Standalone Playwright screenshot capture for the production NAS.
// Usage:
//   MYBIBLI_SESSION_COOKIE="..." \
//   MYBIBLI_REDACT="username1,FirstName,LastName" \
//   node /tmp/mybibli-screenshots.mjs
//
// Run from tests/e2e/ so the @playwright/test module resolves.

import { chromium } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const BASE = "http://bibli.home.arpa";
const COOKIE = process.env.MYBIBLI_SESSION_COOKIE;
const REDACT_RAW = process.env.MYBIBLI_REDACT ?? "";
const REDACT_TERMS = REDACT_RAW.split(",").map(s => s.trim()).filter(Boolean);
const OUT_DIR = "/tmp/mybibli-screenshots";

if (!COOKIE) {
  console.error("ERROR: set MYBIBLI_SESSION_COOKIE");
  process.exit(1);
}

fs.mkdirSync(OUT_DIR, { recursive: true });

const PAGES = [
  { name: "01-home",          url: "/" },
  { name: "02-catalog",       url: "/catalog" },
  { name: "03-series",        url: "/series" },
  { name: "04-locations",     url: "/locations" },
  { name: "05-loans",         url: "/loans" },
  { name: "06-borrowers",     url: "/borrowers" },
  { name: "07-wishlist",      url: "/wishlist" },
  { name: "08-stats-value",   url: "/stats/value" },
  { name: "09-audit",         url: "/audit" },
  { name: "10-admin-health",  url: "/admin?tab=health" },
  { name: "11-admin-system",  url: "/admin?tab=system" },
  // 12-title-detail resolved dynamically from /catalog.
];

const VIEWPORTS = [
  { name: "desktop", size: { width: 1440, height: 900 } },
  { name: "mobile",  size: { width: 390,  height: 844 } },
];

// --- Redaction helpers ---------------------------------------------------

// Replace exact occurrences of `terms` in any text node with █ blocks.
// Runs in the browser context.
async function redactTextNodes(page, terms) {
  if (terms.length === 0) return;
  await page.evaluate((termsArg) => {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const nodes = [];
    let n;
    while ((n = walker.nextNode())) nodes.push(n);
    for (const node of nodes) {
      let v = node.nodeValue;
      let changed = false;
      for (const term of termsArg) {
        if (!term) continue;
        const re = new RegExp(term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
        if (re.test(v)) {
          v = v.replace(re, "█".repeat(Math.max(6, term.length)));
          changed = true;
        }
      }
      if (changed) node.nodeValue = v;
    }
  }, terms);
}

// Blur table body cells on PII-heavy pages (admin users, loans, borrowers).
async function redactPiiTables(page, url) {
  const pii = url.startsWith("/loans") || url.startsWith("/borrowers") || url.includes("tab=users");
  if (!pii) return;
  await page.evaluate(() => {
    document.querySelectorAll("table tbody td, table tbody th").forEach(cell => {
      cell.style.filter = "blur(6px)";
    });
    // Mobile-card surface (UX-DR8 dual-surface): blur card content too.
    document.querySelectorAll("[data-mobile-card] dd, [data-mobile-card] dt").forEach(el => {
      el.style.filter = "blur(6px)";
    });
  });
}

// --- Main ---------------------------------------------------------------

const browser = await chromium.launch();

for (const vp of VIEWPORTS) {
  console.log(`\n=== Viewport: ${vp.name} (${vp.size.width}x${vp.size.height}) ===`);
  const context = await browser.newContext({
    viewport: vp.size,
    deviceScaleFactor: 2,
  });
  await context.addCookies([{
    name: "session",
    value: COOKIE,
    domain: "bibli.home.arpa",
    path: "/",
    httpOnly: true,
    secure: false,
    sameSite: "Lax",
  }]);
  const page = await context.newPage();

  // Resolve a real title id for the detail screenshot.
  let detailUrl = null;
  try {
    await page.goto(BASE + "/catalog", { waitUntil: "domcontentloaded", timeout: 15000 });
    const href = await page.locator('a[href^="/title/"]').first().getAttribute("href").catch(() => null);
    if (href) detailUrl = href;
  } catch (e) { /* fall through */ }

  const allPages = [...PAGES];
  if (detailUrl) allPages.push({ name: "12-title-detail", url: detailUrl });

  for (const p of allPages) {
    try {
      await page.goto(BASE + p.url, { waitUntil: "domcontentloaded", timeout: 20000 });
      await page.waitForLoadState("networkidle", { timeout: 8000 }).catch(() => {});
      await redactTextNodes(page, REDACT_TERMS);
      await redactPiiTables(page, p.url);
      const file = path.join(OUT_DIR, `${p.name}-${vp.name}.png`);
      await page.screenshot({ path: file, fullPage: true });
      console.log(`  ✓ ${p.name}-${vp.name}.png`);
    } catch (e) {
      console.log(`  ✗ ${p.name}-${vp.name}: ${e.message.split("\n")[0]}`);
    }
  }

  await context.close();
}

await browser.close();
console.log(`\nDone. Files in ${OUT_DIR}/`);
