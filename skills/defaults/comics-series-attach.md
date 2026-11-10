Find the comics of this library that belong to an existing series but are not attached to it, and propose to attach them at their position.

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

1. `list_series` (`gaps_only: true` first, then all), then `get_series` for each: read its `version`, its titles with their positions and its expected tomes.
2. Find the library's comic titles of that series that the series does not hold yet (`search_titles` with the series name, then `get_title` to check they are in no series or in the wrong one).
3. A series accepts one pending proposal at a time: file **one** `propose_series_attach` per series with all its missing titles — `series_id`, `series_version`, and `titles` with their positions.
4. Positions come from the publisher's numbering, not from the order of the titles in the library. A title may take the position of an expected tome the series does not hold yet: it then fills that tome. An omnibus covering several tomes uses `is_omnibus` with `end_position`.
5. A position already held by another title of the series: report the conflict, do not file that title.
