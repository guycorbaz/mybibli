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
 * Compute EAN-13 check digit (modulo 10 algorithm).
 * @param first12 - First 12 digits of the ISBN
 * @returns Single check digit character (0-9)
 */
function computeEan13CheckDigit(first12: string): string {
  let sum = 0;
  for (let i = 0; i < 12; i++) {
    const digit = parseInt(first12[i], 10);
    sum += i % 2 === 0 ? digit : digit * 3;
  }
  const remainder = sum % 10;
  return (remainder === 0 ? 0 : 10 - remainder).toString();
}
