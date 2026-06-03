/**
 * Per-spec unique ISBN-13 generator for E2E test data isolation.
 *
 * Each spec file uses a unique 2-character specId to generate ISBNs that
 * never collide with other specs, enabling fullyParallel: true execution.
 *
 * The generated ISBNs have valid EAN-13 check digits and are recognized
 * by the mock metadata server's catch-all handler (returns synthetic metadata
 * for any ISBN not in the known dictionaries).
 */

/**
 * Generate a valid ISBN-13 from a spec identifier and sequence number.
 *
 * Format: 978 + char1code(2d) + char2code(2d) + seq(5d) + checkdigit(1d) = 13 digits
 *
 * @param specId - 2-character unique identifier per spec file (e.g., "CT" for catalog-title)
 * @param seq - Sequence number within the spec (default 1), 0-99999
 * @returns Valid 13-digit ISBN string starting with 978
 */
export function specIsbn(specId: string, seq: number = 1): string {
  if (specId.length !== 2) {
    throw new Error(`specId must be exactly 2 characters, got "${specId}"`);
  }

  const c1 = (specId.charCodeAt(0) % 100).toString().padStart(2, "0");
  const c2 = (specId.charCodeAt(1) % 100).toString().padStart(2, "0");
  const seqStr = seq.toString().padStart(5, "0");

  const prefix = `978${c1}${c2}${seqStr}`;
  // prefix is 12 digits: 978 + 2 + 2 + 5 = 12

  const checkDigit = computeEan13CheckDigit(prefix);
  return `${prefix}${checkDigit}`;
}

/**
 * Per-spec unique location code (L-code) generator (#22).
 *
 * L-codes are validated as exactly `"L"` + 4 ASCII digits
 * (`LocationService::validate_lcode`), so the addressable space is only 4
 * digits. This helper formalizes the convention the specs already follow by
 * hand: the leading digit is a per-spec bucket (1-9) and the trailing 3 digits
 * are a sequence (0-999), e.g. `specLcode(4, 1) === "L4001"`. Pass a bucket
 * that is unique to your spec file to keep labels collision-free under
 * `fullyParallel: true`.
 *
 * @param bucket - Per-spec bucket digit, 1-9
 * @param seq - Sequence within the spec, 0-999 (default 1)
 * @returns A valid 5-character L-code
 */
export function specLcode(bucket: number, seq: number = 1): string {
  if (!Number.isInteger(bucket) || bucket < 1 || bucket > 9) {
    throw new Error(`L-code bucket must be an integer 1-9, got ${bucket}`);
  }
  if (!Number.isInteger(seq) || seq < 0 || seq > 999) {
    throw new Error(`L-code seq must be an integer 0-999, got ${seq}`);
  }
  return `L${bucket}${seq.toString().padStart(3, "0")}`;
}

/**
 * Compute EAN-13 check digit (modulo 10 algorithm).
 * @param first12 - First 12 digits of the ISBN
 * @returns Single check digit character (0-9)
 */
function computeEan13CheckDigit(first12: string): string {
  let sum = 0;
  for (let i = 0; i < 12; i++) {
    const digit = parseInt(first12[i]!, 10);
    sum += i % 2 === 0 ? digit : digit * 3;
  }
  const remainder = sum % 10;
  return (remainder === 0 ? 0 : 10 - remainder).toString();
}
