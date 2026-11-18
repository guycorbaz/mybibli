import { test } from "@playwright/test";

/**
 * Per-spec V-code (volume label) generator for E2E data isolation
 * (story 19-2, D4 — #674, #499, #554).
 *
 * V-codes are `"V"` + 4 ASCII digits and UNIQUE library-wide, so the whole
 * address space is 10 000 codes. It is cut into `VCODE_BLOCK_COUNT` blocks of
 * `VCODE_BLOCK_SIZE` codes, and every spec file that creates a volume owns one
 * block, declared in `VCODE_BLOCKS` below:
 *
 * - the key is the spec's 2-character id (the one it passes to `specIsbn`);
 *   `VcodeSpecId` is the union of the keys, so a typo fails `tsc --noEmit`;
 * - two ids declaring the same block throw when this module loads, so every
 *   spec importing it fails at once;
 * - block 0 is never handed out (it holds V0000, which the server refuses),
 *   and the top block (`HIGHEST_LABEL_CODES_BLOCK`) belongs to
 *   `highest-label-codes.spec.ts`, whose codes must stay above every other
 *   spec's.
 *
 * A block is cut into slots of `VCODE_SLOT_SIZE` codes. The slot is chosen
 * from the running attempt — `repeatEachIndex * (retries + 1) + retry` — so a
 * CI retry or a `--repeat-each` run never re-creates a code its earlier
 * attempt already created on the same database. Codes are deterministic
 * otherwise: a second run on an un-reset stack needs `./scripts/e2e-reset.sh`
 * (already the rule for a truth run).
 *
 * Never hard-code a V-code a spec creates: take it from `specVcode`. A literal
 * V-code may only appear as text no volume is created with (a refusal, a
 * keystroke burst that must be swallowed) — `tests/e2e/vcode-literals.sh`
 * lists the literals and fails on any not allow-listed there.
 */

/** Codes per spec block. */
export const VCODE_BLOCK_SIZE = 200;
/** Number of blocks: `VCODE_BLOCK_COUNT * VCODE_BLOCK_SIZE` = 10 000 codes. */
export const VCODE_BLOCK_COUNT = 50;
/** Codes per attempt slot: `seq` runs 0..VCODE_SLOT_SIZE-1. */
export const VCODE_SLOT_SIZE = 20;
/** The top block, reserved for highest-label-codes.spec.ts. */
export const HIGHEST_LABEL_CODES_BLOCK = VCODE_BLOCK_COUNT - 1;

/**
 * One block per spec file that creates volumes. Block 0 is not assignable;
 * `HIGHEST_LABEL_CODES_BLOCK` (49) is `HC` only. Free blocks: 37–48.
 */
export const VCODE_BLOCKS = {
  LN: 1, // loans.spec.ts
  LR: 2, // loan-returns.spec.ts
  BL: 3, // borrower-loans.spec.ts
  SH: 4, // shelving.spec.ts
  CV: 5, // catalog-volume.spec.ts
  DL: 6, // scan-duplicates-existing-label.spec.ts
  MV: 7, // volume-move.spec.ts
  PV: 8, // catalog-phantom-volume-confirm.spec.ts
  E3: 9, // epic13-smoke.spec.ts
  DC: 10, // dewey-code.spec.ts
  FP: 11, // location-full-path.spec.ts
  VY: 12, // vcode-any-case.spec.ts
  ES: 13, // epic2-smoke.spec.ts
  E6: 14, // epic16-smoke.spec.ts
  CS: 15, // csp-headers.spec.ts
  CM: 16, // catalog-metadata.spec.ts
  VV: 17, // volume-value-proposal.spec.ts
  VA: 18, // volume-page-amounts.spec.ts
  VR: 19, // vcode-label-recovery.spec.ts
  VC: 20, // value-coverage.spec.ts
  VL: 21, // title-detail-volumes.spec.ts
  UC: 22, // title-cover-upload-cta.spec.ts
  MC: 23, // mobile-datatable-cards.spec.ts
  LB: 24, // mcp-label-proposal.spec.ts
  TZ: 25, // instance-time-zone.spec.ts
  HS: 26, // home-search.spec.ts
  E7: 27, // epic17-smoke.spec.ts
  E5: 28, // epic15-smoke.spec.ts
  HL: 29, // admin-health-label-watermarks.spec.ts
  XC: 30, // cross-cutting.spec.ts
  CB: 31, // cover-upload-limits.spec.ts
  CQ: 32, // catalog-cd-sequence.spec.ts
  UH: 33, // home.spec.ts
  TP: 34, // title-volume-add.spec.ts
  EL: 35, // entity-labels.spec.ts
  LP: 36, // labels-page.spec.ts
  HC: HIGHEST_LABEL_CODES_BLOCK, // highest-label-codes.spec.ts
} as const;

export type VcodeSpecId = keyof typeof VCODE_BLOCKS;

/**
 * Load-time check of the block map: every block in range, block 0 unused,
 * the top block for `HC` only, no block declared twice. Exported for a
 * direct call; it runs once when the module loads.
 */
export function checkVcodeBlocks(
  blocks: Readonly<Record<string, number>> = VCODE_BLOCKS,
): void {
  const owners = new Map<number, string>();
  for (const [id, block] of Object.entries(blocks)) {
    if (id.length !== 2) {
      throw new Error(`VCODE_BLOCKS: spec id "${id}" must be exactly 2 characters`);
    }
    if (!Number.isInteger(block) || block < 1 || block >= VCODE_BLOCK_COUNT) {
      throw new Error(
        `VCODE_BLOCKS: block ${block} of "${id}" is out of range 1..${VCODE_BLOCK_COUNT - 1}`,
      );
    }
    if (block === HIGHEST_LABEL_CODES_BLOCK && id !== "HC") {
      throw new Error(
        `VCODE_BLOCKS: block ${block} is reserved for highest-label-codes ("HC"), not "${id}"`,
      );
    }
    const owner = owners.get(block);
    if (owner !== undefined) {
      throw new Error(`VCODE_BLOCKS: block ${block} is declared by both "${owner}" and "${id}"`);
    }
    owners.set(block, id);
  }
}

checkVcodeBlocks();

/**
 * The V-code number for a block, slot and sequence — pure, exported for
 * reasoning about ranges (`highest-label-codes` compares against it).
 */
export function vcodeNumber(block: number, slot: number, seq: number): number {
  const slotsPerBlock = VCODE_BLOCK_SIZE / VCODE_SLOT_SIZE;
  if (!Number.isInteger(seq) || seq < 0 || seq >= VCODE_SLOT_SIZE) {
    throw new Error(`V-code seq must be an integer 0-${VCODE_SLOT_SIZE - 1}, got ${seq}`);
  }
  if (!Number.isInteger(slot) || slot < 0 || slot >= slotsPerBlock) {
    throw new Error(
      `V-code block overflow: attempt slot ${slot} does not fit the ${slotsPerBlock} slots of a ` +
        `${VCODE_BLOCK_SIZE}-code block (slot = repeatEachIndex * (retries + 1) + retry). ` +
        `Lower --repeat-each, or pass --retries=0 with it.`,
    );
  }
  const n = block * VCODE_BLOCK_SIZE + slot * VCODE_SLOT_SIZE + seq;
  if (n <= 0 || n > 9999) {
    throw new Error(`V-code number ${n} is outside V0001..V9999`);
  }
  return n;
}

/** Format a V-code number as the 5-character label (`V` + 4 digits). */
export function formatVcode(n: number): string {
  return `V${n.toString().padStart(4, "0")}`;
}

/**
 * The slot of the running test attempt: `repeatEachIndex * (retries + 1) +
 * retry`. Must be called inside a test (or a hook), like `test.info()`.
 */
export function vcodeSlot(): number {
  const info = test.info();
  return info.repeatEachIndex * (info.project.retries + 1) + info.retry;
}

/**
 * The V-code number `seq` of this spec, in the running attempt's slot.
 *
 * @param specId - the spec's 2-character id, a key of `VCODE_BLOCKS`
 * @param seq - sequence within the spec, 0-19 (one value per created volume,
 *   unique across the spec's tests)
 * @returns e.g. `specVcode("SH", 3)` → `"V0803"` on a first attempt
 */
export function specVcode(specId: VcodeSpecId, seq: number): string {
  return formatVcode(vcodeNumber(VCODE_BLOCKS[specId], vcodeSlot(), seq));
}
