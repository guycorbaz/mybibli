# Story 16.4: Cover upload outside the JSON-RPC call

Status: done

GitHub issue: #587 (type:change-request). Covers: FR169. Epic 16; ships in v1.26.0.

## Story

As an agent proposing covers,
I want to send the image as a file,
so that no image is damaged by being copied character by character.

## Design decisions (dev, 2026-11-06)

- **D1 — route.** `POST /api/v1/cover-uploads` (propose key, multipart, part `file`), recommended by #587 and compatible with threat model § 5.9: nothing is fetched. A 2 MiB + 64 KiB body cap and the MCP 30 s timeout. No session row, like `/api/mcp`.
- **D2 — type.** A part may declare a type or leave it to the file's signature (`curl -F file=@x.jpg` sends one by extension, a script may not); a declared type must match.
- **D3 — table `cover_uploads`.** `token` is the primary key, so a filing locks one row by primary key (Rule #22). A technical row, consumed or purged within the hour: no soft-delete quartet and no foreign key, so the auto-purge order stays untouched.
- **D4 — token.** Bound to its key, single-use, 30 minutes (below `PURGE_GRACE`, 1 hour). Consumed in the filing transaction, so a refused filing keeps it. One same refusal for unknown, expired, used or foreign.
- **D5 — limits.** At most 20 live uploads per key; a new upload removes that key's expired rows and files under the key lock, the others leaving with the purge, so the disk stays bounded. The shared encode semaphore; the encode, staging and recording run in a task owning its permit, so a disconnect neither frees a permit under a running decode nor orphans a file (review, as `FilingJob`). The existing 100 pending covers per key at filing.
- **D6 — receipt.** `sha256`, `width`, `height`, `bytes` describe the file as received, before re-encoding (#587); the item keeps describing the staged JPEG, as for base64.
- **D7 — base64 stays.** `propose_cover` takes exactly one of `upload_token` and `image_base64` + `media_type`, so existing clients keep working. Its description recommends the upload.

## Acceptance Criteria

1. **AC1.** The upload passes the existing checks, is re-encoded and staged; the response gives the token and receipt.
2. **AC2.** `propose_cover` accepts the token as an alternative to base64, exactly one being required; single-use and bound to its key.
3. **AC3.** Unused uploads leave with the staging purge; per-key limits hold; § 5.9 updated.
4. **AC4.** Unit, DB integration (`tests/cover_uploads.rs`) and E2E (`mcp-cover-proposal.spec.ts`): another key's token, an expired token, a damaged image, lock scope.
