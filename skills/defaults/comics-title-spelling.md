Check the spelling of the comics' titles against the published edition, and propose a correction for clear mistakes only.

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

1. Go through the comic titles (`search_titles` with `media_type: "bd"`, page by page), and read each one with `get_title`.
2. Compare its `title` with the edition's title as published: cover, publisher page, BnF record.
3. Propose a correction with `propose_title_metadata` (`title_id`, `fields: {"title": …}`, `sources`) only for a clear mistake: a typo, a missing or wrong accent, an apostrophe or a capital letter that differs from the publication, a truncated title.
4. Do **not** propose: a matter of taste, a translation, another edition's title, or adding or removing a tome number. The tome number lives in the series position (skill `comics-series-attach`), not in the title.
5. One proposal per title, with the source showing the published spelling.
