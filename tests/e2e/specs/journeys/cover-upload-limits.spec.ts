import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";

/**
 * Issue #479 — a cover that is small on the wire and enormous once decoded
 * must be refused, not swallowed.
 *
 * The 10 MiB input cap only ever bounded the *compressed* bytes. A PNG of
 * a couple of hundred bytes can declare an RGBA surface of several
 * gigabytes; before the decode budget landed, the process was killed by
 * the OOM killer — on the Docker deployment, the container dies and takes
 * every in-flight request with it.
 *
 * Spec ID "CB" (cover bomb).
 */

const UPLOAD_BTN = /Upload cover|Téléverser une couverture|Cover hochladen|Carica una copertina/i;

/** CRC-32 (IEEE), needed for a hand-built PNG chunk. */
function crc32(buf: Buffer): number {
  let crc = 0xffffffff;
  for (const byte of buf) {
    crc ^= byte;
    for (let i = 0; i < 8; i++) {
      crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(kind: string, data: Buffer): Buffer {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const typeAndData = Buffer.concat([Buffer.from(kind, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(typeAndData));
  return Buffer.concat([length, typeAndData, crc]);
}

/**
 * A PNG that *declares* `width x height` RGBA and carries no real pixels.
 * The header is all a decoder needs to size its allocation — one that gets
 * as far as reading the pixel data has already lost.
 */
function declaredSizePng(width: number, height: number): Buffer {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // colour type: RGBA
  ihdr[10] = 0; // compression
  ihdr[11] = 0; // filter
  ihdr[12] = 0; // interlace

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk("IHDR", ihdr),
    // Token IDAT so the decoder is constructed at all; never reached.
    pngChunk("IDAT", Buffer.from([0x78, 0x01, 0x01, 0x00])),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

test.describe("Cover decode limits (#479)", () => {
  test("a decompression-bomb cover is refused in the modal, and the app survives", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    const isbn = specIsbn("CB", 1);
    await scanTitleAndVolume(page, isbn, "V0479");

    await page.goto("/?q=" + isbn);
    const detailHref = await page
      .locator('#browse-results a[href^="/title/"]')
      .first()
      .getAttribute("href");
    expect(detailHref).toBeTruthy();
    await page.goto(detailHref!);

    await page.getByRole("button", { name: UPLOAD_BTN }).click();
    const fileInput = page.locator('#modal-slot input[type="file"]');
    await expect(fileInput).toBeVisible();

    // 8000x8000 RGBA = 256 MB decoded, from 200-odd bytes on the wire.
    const bomb = declaredSizePng(8000, 8000);
    expect(bomb.length).toBeLessThan(300);
    await fileInput.setInputFiles({
      name: "bomb.png",
      mimeType: "image/png",
      buffer: bomb,
    });
    await page.locator("#modal-slot [data-modal-confirm]").click();

    // The refusal lands in the modal's own error region (polish-1 AC4.d),
    // and says something the librarian can act on.
    const modalError = page.locator("#modal-slot [data-modal-error]");
    await expect(modalError).toBeVisible({ timeout: 10000 });
    await expect(modalError).toContainText(/too large to process/i);

    // The point of the issue: the process is still there afterwards.
    const health = await page.request.get("/health");
    expect(health.status()).toBe(200);
  });
});
