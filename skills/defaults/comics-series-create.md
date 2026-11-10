Find the comics of this library that belong to a series the library does not have yet, and propose to create that series with its titles.

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

1. For each comic title, `get_title`: note whether it already belongs to a series. For a title in no series, `detected_tome` is a tome number found in the provider's title: a hint for the position, to check against the publisher's numbering.
2. For a title in no series that is clearly part of one (a tome number, a known hero, the publisher's numbering), look the series up with `list_series` (`query` = the series name; also try without accents and without a leading article such as "Les"). If it exists, leave the title to the skill `comics-series-attach`.
3. If no series matches, gather **all** the library's titles of that series (`search_titles` with the series name) and file one `propose_series_create`: the published `name`; `series_type` `closed` with `total_volume_count` only when the series is finished and its number of tomes is certain, otherwise `open`; `titles` with each title's position; and, when you know them, the tomes the library does not own in `expected_tomes`.
4. A series accepts one pending proposal at a time: put everything about a series in that one call.
5. A publisher collection ("Collection Pilote"…) is not a series. A cycle inside a series stays in the series unless the publisher numbers it separately. Two plausible series for a title: report it, do not file.
