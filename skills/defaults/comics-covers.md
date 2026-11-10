Find the comics of this library that have no cover, and propose the cover of the right edition for each.

## Ground rules

- Scope: titles with `media_type` = `bd` only. Pass `"media_type": "bd"` to every list or search tool that takes it.
- You never change the catalog: every change is a proposal the librarian accepts, edits or rejects.
- Every proposal needs `sources`: http(s) URLs where you checked the information (publisher page, BnF catalogue, Bedetheque, the series' Wikipedia page…). No source, no proposal.
- Before filing, call `list_my_proposals` (`status: "pending"`, then `status: "rejected"`). Do not file a proposal that duplicates a pending one or one the librarian rejected: a refusal for that reason is final, so move on.
- When a tool refuses, read its message, fix what it names if you can, otherwise skip that item and report it. Never retry the same call unchanged.
- When you are unsure, do not file: list the case for the librarian with what you found.
- Work in batches of about 20 proposals, then report and ask before going on.

## Report

At the end of each batch: number of titles examined, proposals filed (with their ids), items skipped and why, and the doubtful cases for the librarian to decide.

## Steps

1. `list_titles_without_cover` with `media_type: "bd"`, page by page.
2. For each title, `get_title` to read its ISBN, publisher and publication date.
3. Find the cover of **that edition**: same ISBN, or same publisher and year. Reject an image of another edition, a reprint with a different cover, a fan image, or a picture with a watermark. Two candidate images and no way to choose: report the case, do not file.
4. Send it with `propose_cover` (`title_id`, `sources` = the page the image comes from). Prefer an `upload_token` from `POST /api/v1/cover-uploads` (multipart part `file`) over `image_base64`.
