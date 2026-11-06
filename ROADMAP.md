# mybibli Roadmap

Living plan for what ships next. The version slots are intentional, the
order is broadly stable, and the dates are deliberately absent — mybibli
is a side-project; we ship versions when they're ready, not when a
quarter ends.

The companion site at
[guycorbaz.github.io/mybibli/roadmap.html](https://guycorbaz.github.io/mybibli/roadmap.html)
gives the marketing-friendly view of the same plan. This file is the
canonical, complete one — including the patch-line maintenance policy
and the tech-debt parking lot.

## Versioning policy

mybibli follows [Semantic Versioning](https://semver.org/) 2.0.

- **Patch releases (`1.1.x`)** — bug fixes and known-failure
  resolutions only. No new features land on a patch line, no breaking
  changes, no schema migrations beyond what the fix strictly needs.
  Routine `docker compose pull && docker compose up -d` is always
  enough.
- **Minor releases (`1.2.0`, `1.3.0`, …)** — bundles of new features
  grouped by theme. Backwards-compatible; may ship additive schema
  migrations. The roadmap below is structured around these minors.
- **Major releases (`2.0.0`, …)** — reserved for breaking changes:
  schema migrations that drop columns, route renames, env-var removals.
  None planned at the time of writing — the
  [#206](https://github.com/guycorbaz/mybibli/issues/206) classification
  refactor is the only candidate, and even that may land on a minor if
  we find a backwards-compatible path.

## Now

**Current stable: [`v1.26.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.26.0)**
(2026-11-06) — what real use showed: amounts on the volume page,
purchase prices proposed by the agent, covers sent as files, and a
warning on a code that looks wrong. **Two additive migrations.** The
section below has the detail; every release before it has its own
section further down, and this is the file that carries them — the
README quotes only the current release, and the website tells the same
story for a different audience.

**Next:** the corrections drawn from the project lead's own use, unless
a bug comes first. Later candidates, awaiting prioritisation: more
proposable fields ([#589](https://github.com/guycorbaz/mybibli/issues/589)) and merging duplicates
([#591](https://github.com/guycorbaz/mybibli/issues/591)). The base-path setting for reverse proxies
([#492](https://github.com/guycorbaz/mybibli/issues/492)) comes later. Everything else open lives in the issue
tracker.

## v1.26.0 — what real use showed *(shipped)*

Shipped 2026-11-06. The closing release of epic 16
([#597](https://github.com/guycorbaz/mybibli/issues/597), [#630](https://github.com/guycorbaz/mybibli/issues/630), [#639](https://github.com/guycorbaz/mybibli/issues/639), [#587](https://github.com/guycorbaz/mybibli/issues/587),
[#638](https://github.com/guycorbaz/mybibli/issues/638), [#637](https://github.com/guycorbaz/mybibli/issues/637)), drawn from a first session on v1.25.0
and a reading of its production logs. **Two additive migrations** (the
purchase price thresholds, the `cover_uploads` table), no configuration
change — back up first, pull the image and recreate the container
(`docker compose up -d`).

- **Amounts on the volume page.** Librarians see the purchase price and
  the estimated current value with its date. Anonymous visitors do not.
- **Purchase prices from the agent.** `propose_volume_value` can propose
  the list price at publication without a current value, and can propose
  replacing a purchase price that looks wrong: negligible, or far from
  the list price. Thresholds in *Admin → System → Proposals* (default
  1.00 and a factor of 3). A librarian decides each one. A volume keeps
  two amounts.
- **Covers as files.** `POST /api/v1/cover-uploads` takes the image from
  a `propose` key and answers a single-use token, with the SHA-256 and
  dimensions received, for `propose_cover`. Base64 still works.
- **A warning on an irregular code.** A scanned code that is no known
  barcode shape shows the media-type choice with the reason, even when
  your last choice is remembered. Nothing is created until you choose,
  and the code is kept as entered.
- **Quieter nights.** The retry task logs a `WARN`, not an `ERROR`, when
  the nightly backup makes the database unreachable.
- **Under the hood.** `routes/catalog.rs` and `routes/titles.rs` are
  split below the 2 000-line limit. A blank-browser epic smoke test
  follows a warned code through a cover file and a purchase price.
- **Manual** (EN + FR): *What's new in 1.26.0* in chapter 8; chapters 3
  and 13 cover the new behaviour.

## v1.25.0 — the agent's next tasks *(shipped)*

Shipped 2026-11-05. The closing release of epic 15
([#602](https://github.com/guycorbaz/mybibli/issues/602)), ([#614](https://github.com/guycorbaz/mybibli/issues/614)), ([#590](https://github.com/guycorbaz/mybibli/issues/590)), ([#564](https://github.com/guycorbaz/mybibli/issues/564)).
**Four additive migrations** (a `time_zone` setting, the `mcp_skills`
table, the proposal kinds `series_update` and `volume_value`), no
configuration change — back up first, pull the image and recreate the
container (`docker compose up -d`).

- **Instance time zone** ([#602](https://github.com/guycorbaz/mybibli/issues/602)).
  Admin → System → *Time zone* (an IANA name, default
  `Europe/Zurich`). Pages show dates and cut days — *Added since*,
  overdue loans — in that zone; storage, the REST API and MCP agents
  stay UTC.
- **Skills for the agent** ([#614](https://github.com/guycorbaz/mybibli/issues/614)).
  A new Admin → *Skills* tab where an admin writes Markdown
  instructions (a slug name, a one-line description, a body up to
  64 KiB, an active flag, the trash). Agents receive them as MCP
  prompts (`prompts/list`, `prompts/get`), through the read-only
  `get_skill` tool, and listed in the connection instructions (the
  first 20). A skill grants no capability.
- **Series corrections** ([#590](https://github.com/guycorbaz/mybibli/issues/590)).
  `propose_series_update`: a corrected name, open / closed, the total
  of a closed series, and the removal of expected tomes that do not
  exist. A total below an owned position, or below a live expected tome
  the proposal does not remove, is refused with the positions named.
  The review shows current and proposed values side by side; accepting
  applies everything in one transaction.
- **Volume values** ([#564](https://github.com/guycorbaz/mybibli/issues/564)).
  `propose_volume_value`: a volume's current value in the library's
  default currency (no conversion, two decimals at most) and, only when
  the volume has no purchase price, the list price at publication,
  labelled as a stand-in. `get_title` now gives each volume's edition
  note, purchase price and current value with its date, and the default
  currency.
- **Value coverage** ([#564](https://github.com/guycorbaz/mybibli/issues/564)).
  `/stats/value` opens with the coverage of the estimates — live
  volumes, valued, share, oldest and newest estimate, estimates older
  than a year or undated — overall, per genre and per series (least
  covered first). The read tool `list_volumes_to_value` lists the
  volumes never valued first, then the oldest estimates (filters
  `series_id`, `genre`).
- **Fixes.** The per-currency totals no longer count volumes of deleted
  titles; the per-series totals count an omnibus once and ignore
  detached series assignments.
- **MCP.** 18 tools (new: `get_skill`, `propose_series_update`,
  `propose_volume_value`, `list_volumes_to_value`) and the `prompts`
  capability. Minor review findings deferred:
  [#627](https://github.com/guycorbaz/mybibli/issues/627),
  [#628](https://github.com/guycorbaz/mybibli/issues/628),
  [#630](https://github.com/guycorbaz/mybibli/issues/630),
  [#632](https://github.com/guycorbaz/mybibli/issues/632).
- **Under the hood.** A blank-browser epic smoke test follows a skill
  served to an agent, a series modification and a volume value from
  filing to acceptance.
- **Manual** (EN + FR): *What's new in 1.25.0* in chapter 8; chapters
  2, 3 and 13 cover the time zone, the Skills tab, the new proposals
  and the value coverage.

## v1.24.0 — the sidebar *(shipped)*

Shipped 2026-11-04. The closing release of epic 14
([#547](https://github.com/guycorbaz/mybibli/issues/547)): the top
navigation bar no longer fitted an admin's entries in French or German.
**No migration, no setting changed** — pull the image and recreate the
container (`docker compose up -d`).

- **Sidebar from 1024 px.** Every entry with its icon and label,
  grouped (Home · Catalog, Locations, Series · Borrowers, Loans, Wish
  list, Labels, Proposals · Admin, per role), the current page marked.
  Language, theme, log out and a **Collapse menu** button sit at its
  foot. Collapsed, it shrinks to a 64 px column of icons with tooltips,
  giving the width back to the tables; the Proposals badge becomes a
  pill on its icon. The browser remembers the choice.
- **Drawer below 1024 px** (phones and tablets). A slim top bar with
  the menu button opens the same navigation from the left, over a
  dimmed page; Escape, the dimmed page, the close button or a link
  close it. While it is open, the page behind does not scroll and a
  barcode scanner's read is ignored.
- **Home entry** at the top of the navigation (the logo still links
  home too).
- **The theme toggle really switches light / dark** — before, the
  whole interface followed the operating system and the toggle only
  changed the home-page cards. The browser remembers the choice;
  without one, mybibli follows the system. Printing is always light.
- **Fixes.** With browser storage blocked, the theme toggle, the scan
  sound and the list / grid toggle fall back to their defaults instead
  of breaking the page; the session-timeout and connection-lost notices
  sit at the bottom right again; the error page follows the chosen
  theme.
- **Under the hood.** One navigation source for the sidebar and the
  drawer; dark mode is class-based and screen-only; Tailwind now scans
  `static/js` (vendor files excluded). Playwright: 443 passed
  (2 skipped) at `--workers=2`, with new sidebar, drawer, responsive
  (360 → 1023 px × four locales × three roles) and blank-browser epic
  smoke tests.
- **Manual** (EN + FR): *What's new in 1.24.0* in chapter 8; chapter 3
  (the sidebar, the drawer, the theme) and chapters 2, 9, 11, 13 with
  new screenshots in each language.

## v1.23.0 — cataloguing for real *(shipped)*

Shipped 2026-11-04. The closing release of epic 13, planned from the
first mass entry session (2026-10-31 → 11-02, about 260 volumes), its
production logs and an agent session on the comics (371 proposals
filed). **Five additive migrations** (`metadata_fetch_retries`,
`data_repairs`, `titles.detected_tome`, a `label_attach` proposal kind
and a `volumes` proposal target) — back up first.

- **Scanning a pile**
  ([#574](https://github.com/guycorbaz/mybibli/issues/574),
  [#585](https://github.com/guycorbaz/mybibli/issues/585)). A double
  read within one second is ignored; re-scanning a V-code says
  "already recorded" or names the title that holds it, with a link; a
  failed shelving or a deleted active location is reported.
- **Throttled lookups retried**
  ([#583](https://github.com/guycorbaz/mybibli/issues/583)). A lookup
  that met a saturated provider goes on a retry queue, retried after
  15 min, 1 h, 4 h, 12 h and 24 h.
- **ISBN-10 stored as UPC**
  ([#584](https://github.com/guycorbaz/mybibli/issues/584)). At every
  start, printed titles catalogued before v1.21.0 with an ISBN-10 get
  their ISBN-13 and are looked up again through the retry queue; a
  collision with an existing ISBN-13 is logged until resolved by hand.
- **Plain-text descriptions**
  ([#578](https://github.com/guycorbaz/mybibli/issues/578)). HTML from
  Google Books, Comic Vine or an agent is converted to text; stored
  descriptions are repaired once at the first start (hand-edited ones
  untouched).
- **Move a volume to another title**
  ([#580](https://github.com/guycorbaz/mybibli/issues/580)) from its
  page; a loan follows the volume.
- **Dates added**
  ([#586](https://github.com/guycorbaz/mybibli/issues/586)). *Added on*
  / *Modified on* on title and volume pages; an *Added since* filter
  and an *Added* column and sort on the home page; *Recent cataloged*
  counts titles that received a volume. Days are cut in UTC
  ([#602](https://github.com/guycorbaz/mybibli/issues/602)).
- **Lists**: *Recent additions* follow the List/Grid toggle
  ([#577](https://github.com/guycorbaz/mybibli/issues/577)); the
  title-page series selector lists every series
  ([#576](https://github.com/guycorbaz/mybibli/issues/576)); volume
  lists show full location paths
  ([#582](https://github.com/guycorbaz/mybibli/issues/582)).
- **Tome numbers**
  ([#579](https://github.com/guycorbaz/mybibli/issues/579)). "Tome N" /
  "Volume N" display, inline position edit, a detected number as a hint;
  tome markers stripped from provider titles at (re-)download unless
  the title was edited by hand.
- **Agents: 14 MCP tools.** `list_labels` and `propose_label` — an
  agent proposes a management label on a doubtful title or volume
  ([#581](https://github.com/guycorbaz/mybibli/issues/581)); `get_title`
  and `search_titles` return dates, location paths, labels and the
  detected tome; an owned title may fill an expected tome by proposal
  ([#588](https://github.com/guycorbaz/mybibli/issues/588)); agent
  descriptions are converted to plain text at filing.
- **API key revocation** retried on a database deadlock instead of a
  server error ([#554](https://github.com/guycorbaz/mybibli/issues/554)).

Upgrade notes: the first start runs the one-time description repair
and the ISBN-10 conversion (both logged); titles left empty by a
saturated provider before the upgrade can simply be re-downloaded;
tome markers are stripped only on (re-)download — titles already
catalogued are not cleaned in bulk, and a real title shaped like a
marker ("L'Ennemi public n°1") needs one hand edit. Monitoring tip: point
an uptime or Prometheus/blackbox probe at `/health`, not `/` — every
cookieless request to `/` creates an anonymous session.

## v1.22.0 — proposals, after a first real use *(shipped)*

Shipped 2026-11-01. The closing release of epic 12, planned from the
first production agent session (2026-10-30: 352 proposals filed, 245
accepted, 190 titles improved). **No migration.**

- **Review in any order — staleness per field**
  ([#542](https://github.com/guycorbaz/mybibli/issues/542)). Accepting a
  cover no longer outdates a metadata proposal on the same title, nor the
  reverse. A proposal goes stale only when something it touches changed
  (a field, the cover), its title was deleted or re-identified by hand,
  or a proposed contributor role was deleted. Every cover write now
  stores a cache-busted URL.
- **Rejections by field and by image**
  ([#535](https://github.com/guycorbaz/mybibli/issues/535)). A rejected
  metadata proposal blocks each of its fields for the resubmission
  delay; a rejected cover blocks only the same image, so another image
  can be proposed at once.
- **The agent is told where mybibli already searched**
  ([#560](https://github.com/guycorbaz/mybibli/issues/560)). The MCP
  server's instructions list, per media type, the providers mybibli
  queries and how the lookup stops, so an agent looks elsewhere first —
  those sources may hold the record now.
- **The title can be proposed**
  ([#563](https://github.com/guycorbaz/mybibli/issues/563)). An agent can
  correct a title's main title (spelling, or the real title of a record
  named after its ISBN); an accepted correction keeps the other pending
  proposals valid.
- **A readable proposals list**
  ([#561](https://github.com/guycorbaz/mybibli/issues/561)). The subject
  opens the proposal, "View title" leads to the catalog, a line lists the
  proposed fields and a pending cover shows a thumbnail.
- **ISBN typed with hyphens or spaces**
  ([#502](https://github.com/guycorbaz/mybibli/issues/502)) is recognised
  at scan and in search.
- **Truncated JPEG covers refused**
  ([#558](https://github.com/guycorbaz/mybibli/issues/558)) — the fix
  first prepared as v1.21.1, which was withdrawn before any image was
  published.

Upgrade notes: proposals still pending at the upgrade keep the previous
staleness rule — have the agent file them again to benefit; metadata
rejections from before the upgrade now block per field; cover
rejections from before block nothing new.

## v1.21.0 — AI agents, under supervision *(shipped)*

Shipped 2026-10-29. The closing release of epic 11
([#493](https://github.com/guycorbaz/mybibli/issues/493)), whose three
leading fixes already shipped in v1.20.1. **Six additive migrations**
(the `propose` key scope, proposals and their items, expected tomes,
the equivalence key, the series a creation produced, line edits); no
existing column changes.

- **MCP server at `/api/mcp`.** An AI agent — Claude Code, Hermes Agent
  or any MCP client over streamable HTTP — connects with a new
  *Read + propose* API key (`mybibli_pr_…`, minted in Admin → API keys)
  and sees twelve tools: eight read tools (`search_titles`, `get_title`,
  `search_contributors`, `list_series`, `get_series`,
  `list_titles_without_cover`, `list_titles_with_incomplete_metadata`,
  `list_my_proposals`) and four propose tools (`propose_series_create`,
  `propose_series_attach`, `propose_title_metadata`, `propose_cover`).
  Every proposal cites its sources; the agent never changes the
  catalog. Built on `rmcp` pinned at `=3.8.0`, confined to `src/mcp/`.
  **LAN only:** requests are refused unless their `Host` is in
  `MYBIBLI_MCP_ALLOWED_HOSTS` (default `localhost,127.0.0.1,::1`) and
  any `Origin` is in `MYBIBLI_MCP_ALLOWED_ORIGINS` (default empty) —
  DNS-rebinding defence, deployment settings on purpose.
- **Proposals page (`/proposals`).** A navigation entry with a
  pending-count badge and a home-page indicator for Librarians and
  Admins. List with status and kind filters; detail with the sources,
  current and proposed values side by side (*New* / *Changed* badges),
  current and proposed cover. Lines can be edited or removed before
  accepting (with an audit trail); accept applies under locks and
  re-checks the live catalog — a proposal whose subject changed is
  marked *Outdated* and applies nothing. Accepted fields and covers
  count as edited by hand, so automatic metadata downloads never
  overwrite them. Reject takes an optional reason the agent reads back.
- **Resubmission delay.** After a rejection, an equivalent proposal is
  refused library-wide for 90 days by default (Admin → System →
  Proposals, 0–3650 days).
- **Expected tomes.** A series can name the tomes you do not own yet —
  by hand or from an accepted proposal. They fill their gap on the
  series page, go to the wish list in one click, and take their
  position automatically when the book is catalogued.
- **Cover staging.** A proposed cover is validated and re-encoded, then
  waits in `<COVERS_DIR>/.staging` on the existing covers volume — never
  served by `/covers`, shown to librarians only while pending, cleaned
  up by the scheduled purge. The server never downloads an
  agent-supplied URL. Revoking a key marks its pending proposals
  outdated and deletes their staged covers.
- [#496](https://github.com/guycorbaz/mybibli/issues/496) — **the
  navigation bar stays at the top** while scrolling, and switches to
  the hamburger menu below 1280 px (was 1024 px) so every entry fits
  on one line in all four languages.
- **Documentation.** New manual chapter 13, "AI agents and proposals"
  (EN + FR): connecting an MCP client, the tools, the review workflow,
  expected tomes, staging, and an MCP Inspector check for operators.

## v1.20.1 — three production fixes *(shipped)*

Shipped 2026-10-27. Three bug fixes from epic 11, **no migration**, no
setting change.

- [#497](https://github.com/guycorbaz/mybibli/issues/497) — **an
  ISBN-10 is recognised at scan.** Scanned on `/catalog` or on the home
  page, a ten-character ISBN used to be taken for a UPC and created a
  title with no metadata. It is now validated (trailing `X` included)
  and converted to its ISBN-13 — `2205001213` → `9782205001211` — for
  the duplicate check, the title row, the wishlist link and the
  provider chain; a home-page search by ISBN-10 finds the title too.
  One helper in `src/utils.rs` serves scan and search. Titles already
  catalogued under a raw ISBN-10 are **not** merged automatically: the
  owner cleans them up, and until then rescanning such a book creates a
  second title under the ISBN-13.
- [#495](https://github.com/guycorbaz/mybibli/issues/495) — **the
  Library of Congress answers again.** Since 2026-10-26 its JSON search
  (`www.loc.gov/books/?fo=json`) sits behind a Cloudflare bot challenge
  and the provider returned nothing. The provider now reads MARC 21
  through the SRU endpoint (`lx2.loc.gov`) that already supplied the
  six cataloguing zones since v1.14.0: title, subtitle, contributors,
  publisher, date, language, page count, description and Dewey, entity-
  decoded and NFC-normalised. The JSON path is removed; a challenge is
  logged as such and never retried. **Upgrade note:** SRU is reached
  over plain HTTP on **port 210** — a network that allows only outbound
  80/443 shows the Library of Congress as unreachable. English-language
  titles scanned since 2026-10-26 got no LoC data: use *Re-download
  metadata* on the title, or *Backfill metadata from libraries* on the
  Health tab.
- [#494](https://github.com/guycorbaz/mybibli/issues/494) — **the
  *Recent additions* triangle rotates** when the home-page section is
  open, including when the open state is restored from the last visit.
  CSS only.
- **Dependency fix** — `rustls` 0.23.45 → 0.23.46 and `rustls-webpki`
  0.103.15 → 0.103.16 for RUSTSEC-2026-0346, -0347, -0348, -0349 and
  [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382),
  caught by the daily `cargo audit`. Lockfile only.

## v1.20.0 — where the labels end *(shipped)*

Shipped 2026-09-23. One change request, **no migration**.

- [#489](https://github.com/guycorbaz/mybibli/issues/489) — **the admin
  Health tab shows the last volume number (V-code) and the last shelf
  number (L-code) in use, each with the next free number** —
  `V0142 (next: V0143)`. The librarian prints sheets of pre-generated
  barcode labels in advance and needs to know where the occupied range
  ends; since v1.12.0 that answer lived only on the `/catalog` info line
  ([#428](https://github.com/guycorbaz/mybibli/issues/428)). "Last" is
  the highest number ever printed, trashed items included, so a sticker
  already stuck on a book or a shelf is never reissued — the same rule
  `/catalog` follows, so the two surfaces never contradict each other.
  An empty catalog reads "none yet"; `V9999` reads "none left".
- **Dependency fix** — `rustls` 0.23.40 → 0.23.45 for
  [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285)
  (TLS 1.3 handshake messages accepted across encryption-level
  boundaries; medium), caught by the release-cut `cargo audit`. Affects
  the outbound HTTPS client used for metadata lookups; no code change.

## v1.19.0 — the security review *(shipped)*

Shipped 2026-09-11. Three change requests, **no migration**, one
documentation sweep. The review that produced them was run on 2026-08-18
against the v1.18.0 code, with a stated attacker model: local network
plus a private tunnel, never a port forwarded to the internet.

- [#478](https://github.com/guycorbaz/mybibli/issues/478) — **the Trash
  panel's Restore button reaches a route that exists.** It had rendered
  a link to `/admin/trash/{table}/{id}/restore` since story 8-6; that
  route was never registered, so every click returned a 404 that HTMX
  does not swap — no restore, no error, nothing at all. The route is
  registered as POST rather than the GET the button emitted, so it rides
  the CSRF layer; where restoring would clash with a relationship formed
  since the deletion, a modal names the conflict before clearing it.
  The suite had stayed green throughout because `TrashService::restore`
  was only ever called from `#[cfg(test)]`.
- [#480](https://github.com/guycorbaz/mybibli/issues/480) — **the seed
  gate hard-deletes its artefacts.** It had soft-deleted the seeded
  `admin` / `librarian` rows, leaving them in the Trash for thirty days
  with the passwords published in `SECURITY.md` intact, plus a session
  row whose token is in the git history. Harmless while Restore was
  broken; #478 is what made it urgent, and the two had to land in that
  order. Upgrading instances purge whatever their old gate left behind.
- [#479](https://github.com/guycorbaz/mybibli/issues/479) — **a cover
  decode runs under an allocation budget.** The 10 MiB cap bounded the
  compressed bytes, not the memory: a few hundred bytes of PNG can
  declare gigabytes of RGBA, and the OOM killer would take the container
  down with every request in flight. 64 MiB and a 10 000 px per-side
  ceiling; the download side now streams with a hard cap instead of
  trusting `Content-Length`. The path that matters is the provider
  chain, where the bytes are not ours.

Also in this release: the CI actions are pinned by commit SHA with a
daily RustSec advisory scan ([#481](https://github.com/guycorbaz/mybibli/pull/481)),
the network posture is stated where an operator will actually read it
([#482](https://github.com/guycorbaz/mybibli/pull/482)), the
community-health files landed
([#477](https://github.com/guycorbaz/mybibli/pull/477)), and a
documentation audit closed fourteen findings across every surface
([#487](https://github.com/guycorbaz/mybibli/pull/487)) — K10plus finally
documented, the shelf-audit chapter describing the feature that shipped
three releases ago rather than a workaround, a route reference covering
all 148 routes instead of 82.

## v1.18.0 — management labels *(shipped)*

Shipped 2026-08-18. One issue, **one additive migration**, three reviewed tranches.

- [#443](https://github.com/guycorbaz/mybibli/issues/443) — **a vocabulary for the work in progress.** Genres classify a book for the reader; volume states describe a copy's condition. Neither says "I am not sure this record is right, come back to it". Labels do: an admin defines the vocabulary in *Administration → Reference data*, and a librarian puts any of it on a title or on a volume, from the same list. A `/labels` page shows the vocabulary with its usage, drills down to the members of one label, and links through to the item so it can be fixed.

  Three things about the design are worth recording, because each was a decision rather than a default:

  **Usage is the sum of two join tables.** A label spans titles and volumes, so the deletion guard counts both. A guard looking at one of them would report a plausible zero for a label carried by fifty volumes and delete it off every shelf without a word — the failure mode that shaped the whole model layer, and the one the integration tests exercise in each direction.

  **The member list is two sections, not one merged table.** A title row wants cover / title / contributor / genre; a volume row wants V-code / parent title / location / state. Merging them would either duplicate columns or flatten to their intersection. Each section pages independently, and an empty one states its emptiness rather than disappearing — "no volumes carry this label" has to stay distinguishable from "that section failed to load".

  **Anonymous visitors are excluded by not asking.** The detail handlers skip the label queries entirely rather than loading them and hiding them, so there is no code path on which a label name reaches a visitor. The nav entry sits inside the Librarian+ block, and the endpoints refuse rather than merely hiding the affordance.

  Two pre-existing guards caught defects in this work that review had not: the #424 contrast audit rejected `text-stone-400` in the new fragment and named the correct tier, and `responsive-layouts.spec.ts` measured an 86px tablet overflow from the new nav entry. **The nav bar therefore switches to its menu at 1024px instead of 768px** — a deliberate change affecting every role, chosen over hiding the single entry, which would have left `/labels` reachable by URL only between 768 and 1023px.

## v1.17.0 — metadata clarity *(shipped)*

Shipped 2026-08-18. Two issues, **no migration**.

Both came from the same production user, reported eight months apart, and auditing each one before writing code changed what got written. Neither turned out to be the defect it was filed as; both were the interface failing to say something true.

- [#202](https://github.com/guycorbaz/mybibli/issues/202) — **a failed lookup now says why.** The chain has distinguished every failure cause since v1.11.0 — no result, throttled, unavailable, network failure, per-provider timeout — but each one went to the log and reached the user as a single "no metadata found". That one message covered three situations calling for opposite reactions: nobody holds the book (catalogue it by hand; no key and no retry will help), a source was busy or too slow (worth pressing again in a few minutes), or a source was never asked for want of an API key (one field in Administration → System). Pressing *Re-download metadata* now names which, and names the sources — untranslated, as in the v1.15.0 provenance badge. No new detection logic was written: the diagnosis existed, only its retention was missing.

  One conflation had to be repaired at the source. OMDb and TMDb, given no API key, returned the same "found nothing" as a genuine empty search — the very distinction this work exists to draw. They now report the missing key as such.

  The diagnosis is produced on demand rather than stored: provider availability is episodic, so a reason recorded three weeks ago would describe a world that no longer exists — and persisting it would have cost a migration for a worse answer. **Tier 3 (retrying one specific provider) was dropped**: once the message names the source and the reason, a targeted retry does what the ordinary one does, with one more control to learn.

- [#206](https://github.com/guycorbaz/mybibli/issues/206) — **which of genre and Dewey a fetch may overwrite.** The report asked that the genre stop being overwritten automatically. It already was not, three times over: no provider returns a genre, `genre_id` is absent from the fetch UPDATE, and the edit-tracking helper excludes it. There was no defect. What the form never said is that the neighbouring field behaves the *opposite* way — the Dewey code **is** provider-filled, and keeps being so until the first manual edit records the value as yours. Read from the form alone, that asymmetry inverts, which is very likely how the report came to be written.

  Both fields now sit in a **Classification** section stating the difference; publication date, being bibliographic, stays outside it. A guard in the code fails the build if a metadata fetch ever writes the genre — the interface makes a promise, and nothing else was enforcing it.

## v1.16.0 — K10plus bibliographic coverage + admin recovery *(shipped)*

Shipped 2026-08-17. Four issues, **no migration**, two release candidates.

- [#450](https://github.com/guycorbaz/mybibli/issues/450) — after v1.14.0 added the Library of Congress, 38 English-prefix titles still carried none of the six UNIMARC-aligned fields. Six catalogues were probed before one qualified: Jisc Library Hub is licence-gated (403 anonymously), the British Library's SRU and linked-data endpoints no longer answer at all, Harvard LibraryCloud holds none of them, Open Library was already ruled out. **K10plus**, the German union catalogue, answers anonymous SRU with MARC 21 XML under CC0 — the cleanest licence in the chain — and reads through the existing `metadata::marc` parser, so it needed only a provider declaring `supplies_marc_zones()`. It recovered 18 of the 38, every one of them, and 45 titles / 100 zones across the whole catalog. The requests are gated by ISBN prefix (978-0/1/2/3) so the 1 req/s limiter only paces calls that can plausibly hit; the 978-2 gate acts as a free second opinion on titles the BnF answered without zones, and earned its place with 9 completions.
- [#459](https://github.com/guycorbaz/mybibli/issues/459) — an administrator locked out of a single-tenant install previously needed a MariaDB console to get back in. `MYBIBLI_RESET_ADMIN` reads once at startup, resets exactly one account, and is consumed — leaving the variable set in `docker-compose.yml` does not re-arm it.
- [#457](https://github.com/guycorbaz/mybibli/issues/457) — the next-free L-code proposal ignored soft-deleted rows, so it could propose a code a deleted location still held and the save then failed on the UNIQUE constraint. It now counts deleted rows and proposes the lowest genuinely free code.
- [#458](https://github.com/guycorbaz/mybibli/issues/458) — four E2E specs failed intermittently, which is worse than failing outright: it trains the reader to re-run rather than to read. Three distinct root causes, all fixed at the source rather than papered over with waits.

**On the two release candidates.** rc.1 was withdrawn before publication: its own release gate caught a rate-limiter contention bug — the trait-exposed 1 req/s limiter serialised the zone-completion pass under parallel load, 82 K10plus calls per E2E run. rc.2 added the prefix gate and shipped to production for measurement. Its backfill log then settled the decision, and did so without the database: the `Title metadata resolved` event carries `unimarc_zones_populated`, so the affected population reconstructs from the log files alone — the 29 July baseline of 38 was reproduced 19 days after the fact, validating the instrument before it was trusted on the new run. The pre-registered acceptance threshold (19 of 38) was missed by a single title; it was overridden deliberately, because it scored the provider solely on the hardest population in the catalog and ignored the 27 further titles it completed elsewhere. The 20 titles that remain empty cluster by publisher — two UK small presses and a scatter of print-on-demand imprints — and are recorded on the issue as not automatically recoverable by any catalogue source probed.

## v1.15.0 — metadata provenance + accessibility *(shipped)*

Shipped 2026-08-13. Four issues, **one additive migration**.

The bundle came out of a review of the production logs on the household NAS covering 14 July to 13 August: 30 days of daily rotation, one process, no restart since the v1.14.1 deployment.

- [#202](https://github.com/guycorbaz/mybibli/issues/202) — the provider chain always knew which provider answered and then threw it away, so a librarian facing a thin record could not tell "the BnF answered but holds little" from "nothing answered at all". Titles now carry `metadata_source` and the detail page names it. The provider name is not translated — it is a proper noun. Tier 1 of three: the structured failure surface and per-provider retry stay open on the issue, and both depend on this one. A cache hit resolves metadata without naming a provider, so the write is skipped rather than erasing a previously recorded source.
- [#424](https://github.com/guycorbaz/mybibli/issues/424) — `text-stone-400` sat at 2.41:1 on the page background, below the WCAG 2.2 AA floor of 4.5:1. Measuring first changed the fix the issue proposed: `text-stone-500` still fails at 4.40:1 on the hover shade the locations rows take, so those controls went to `text-stone-600`. The home badge turned out to fail in **both** themes, not just light. Enforcement is now two-level — a static template audit plus a spec that seeds data before running axe, because both offending elements only render when the catalog is not empty and CI had been passing on scheduling luck.
- [#419](https://github.com/guycorbaz/mybibli/issues/419) — a third retry tier (60 s) for bulk metadata runs. Production evidence: of 25 throttled titles, 12 were rescued at 5 s and 4 at 20 s, leaving 9 written off with the schedule exhausted. The measurements also settled a standing hypothesis — **spacing requests further is not the lever**: the median gap before a 503 (4.24 s) and before a success (4.19 s) are indistinguishable, and the 503 rate does not follow the call rate. Roughly one unauthenticated Google Books call in two fails during a storm, independent of pacing. Per-title, so a clean run pays nothing.
- [#449](https://github.com/guycorbaz/mybibli/issues/449) — the startup line carried the full 40-character commit while its own documentation called it short. Shortened at emission, keeping the full hash on the struct for any future surface.

Upgrade: image swap plus one automatic additive migration. Rolling back to v1.14.1 is clean — the new column is nullable and unread by the previous binary.

## v1.14.1 — build identity in the log *(shipped)*

Shipped 2026-07-28. One issue, **no migration**.

- [#447](https://github.com/guycorbaz/mybibli/issues/447) — the startup log line now carries the version, the git commit the image was built from, and the build profile. Confirming which release is running previously meant leaving the log file for the host's `docker-compose.yml` — a detour made worse by releases with no migration, which leave no other trace to infer the version from. The commit is stamped in by CI as a Docker build argument; a locally-built binary reports `unknown` rather than claiming a commit it cannot verify.

## v1.14.0 — cataloging fixes + Library of Congress MARC 21 *(shipped)*

Shipped 2026-07-28. Four issues, **no migration** — reuses the columns added by #389, so the upgrade is an image swap against an unchanged schema and rolling back to v1.13.0 stays clean.

- [#440](https://github.com/guycorbaz/mybibli/issues/440) — `severity:high`. Cataloguing several UPC-identified items (typically CDs) in a row left the previous title active after the first, so the next volume label attached to the wrong item or failed with a misleading "not found". All scan paths now share one context activation.
- [#441](https://github.com/guycorbaz/mybibli/issues/441) — a V-code scan whose active title had been deleted reported "not found", blaming the label. It now reports that no item is active and clears the stale context.
- [#442](https://github.com/guycorbaz/mybibli/issues/442) — deleting a volume left its V-code locked until an admin purged it from the Trash, and the error named the owner "?". The label is now reusable; the previous copy's details are discarded and audited. Refused when the volume carries loan history, which reuse would re-attribute to a different copy.
- [#439](https://github.com/guycorbaz/mybibli/issues/439) — the Library of Congress provider now also fetches MARC 21 records over SRU, filling the six UNIMARC-aligned fields for English-language books. The two national libraries complete each other field by field, first source wins. Measured reach: ISBN prefixes 9780/9781 only — **French-prefix titles the BnF could not describe stay empty**. The Health-tab action is renamed *Backfill metadata from libraries*.

## v1.13.0 — UNIMARC-aligned cataloging *(shipped)*

A UNIMARC-themed feature minor on v1.12.0 (shipped 2026-07-24). One automatic, additive migration; no operator action required (the backfill below is optional and on-demand).

- [#389](https://github.com/guycorbaz/mybibli/issues/389) — **UNIMARC-aligned cataloging fields (Palier 1).** Titles carry six new bibliographic fields mapped to their UNIMARC zones: statement of responsibility (200$f), edition statement (205$a), collection title + number (225$a/$v), general note (300$a), original title (500$a). Captured automatically from the BnF at scan time, rendered on the title page when present, never overwriting manually edited values. The authoritative field⇄zone mapping ships as [`docs/unimarc-mapping.md`](docs/unimarc-mapping.md). A new Health-tab bulk action — **Backfill metadata from BnF** — re-runs the lookup for every coded title to fill the new fields on catalogs built before the upgrade (shares the pacing/back-off/lock of the cover refetch). Palier 2 — record import/export (ISO 2709 `.mrc` / UNIMARC XML) — was dropped on 2026-08-17: SIGB record exchange makes no sense for a household library, so the UNIMARC work stops at the internal-model alignment.
- [#434](https://github.com/guycorbaz/mybibli/issues/434) — **Cataloging log summary.** Each metadata resolution logs one `info` summary line (cover resolved? how many UNIMARC fields filled?), making backfill runs auditable from the log file.

## v1.12.0 — cover sources + label helper *(shipped)*

A 2-issue feature minor on v1.11.0 (shipped 2026-07-11, same day). No migration, no operator action.

- [#427](https://github.com/guycorbaz/mybibli/issues/427) — **Two new cover sources.** When metadata resolves without a cover, the chain now tries the *BnF Couvertures* service (legal-deposit cover scans — the strongest source for FR/CH publishers, print-quality images) and *Inventaire.io* (libre community database, strong on FR comics/manga) before the Open Library fallback. Live-tested against the production catalog: 53 of the 112 missing covers (47%) become recoverable. Also fixes a latent bug where covers served from non-TLS hosts could never download (unconditional http→https rewrite).
- [#428](https://github.com/guycorbaz/mybibli/issues/428) — **Label-printing helper.** /catalog shows the highest V-code and L-code already in use (trash included — a printed sticker is never reissued), so fresh label sheets start after them.

## v1.11.0 — production-log review bundle *(shipped)*

A 4-issue minor on v1.10.0 (shipped 2026-07-11), driven end-to-end by the 2026-07-10 production-log review. One additive settings migration (seeds the bulk-refetch delay row); applies automatically — no operator action.

- [#418](https://github.com/guycorbaz/mybibli/issues/418) — **Sessions survive a tablet screen-lock.** The session cookie is now persistent (lifetime = the inactivity timeout) instead of a browser-session cookie, so iPads that discard session cookies on lock no longer log the librarian out mid-cataloguing. The inactivity timeout is admin-configurable in hours (1–720, default 4) under a new /admin > System > Sessions section.
- [#419](https://github.com/guycorbaz/mybibli/issues/419) — **Bulk cover-refetch actually recovers covers.** The *Re-fetch missing covers* action paces its requests (new `bulk_refetch_delay_ms` setting, default 1 s/title), retries 429/503-throttled lookups with 5 s → 20 s back-off (new typed 503 signal end-to-end through the provider chain), and reports a completion summary on the Health tab — covers recovered / provider errors / no cover available. Two prod runs over ~113 titles had previously tripped Google Books' 503 burst-throttling and silently recovered ~0.
- [#416](https://github.com/guycorbaz/mybibli/issues/416) — **Daily auto-purge unblocked.** Orphan `sessions` rows referencing a >30-day-soft-deleted user (any path outside the story 8-3 deactivation handler: seed state, direct SQL) blocked the user hard-purge forever via the RESTRICT FK — a daily ERROR since 2026-06-20. The purge now wipes those rows in the same transaction; the counts surface in the audit payload.
- [#417](https://github.com/guycorbaz/mybibli/issues/417) — **Log-noise fix.** The dashboard Gaps chips (`uncategorized` / `no_volumes` / `no_cover`) are valid search-side filters; the indicator-filter parser now ignores them silently instead of WARN-logging every click (genuinely unknown values still warn).

## v1.10.0 — undo recent scan actions *(shipped)*

A single-feature minor on v1.9.1 (shipped 2026-07-01). No migration, no operator action.

- [#9](https://github.com/guycorbaz/mybibli/issues/9) — **Undo a scan action.** When a librarian shelves a volume or activates a batch storage-location on the catalog page, the feedback message now carries an **Undo** button. Clicking it within a server-authoritative 30-second window reverses that last action: a shelving returns the volume to its previous location (or leaves it unshelved), and a batch-location activation is reverted. Only the most recent action is undoable, and undo never deletes a title or volume that was just created. Implemented via a per-session undo log in the existing `sessions.data` blob (no schema change) + a new `POST /catalog/undo` endpoint; the button is CSP-clean and CSRF-protected like every other state-changing request.

## v1.9.1 — i18n sweep + CI de-flake *(shipped)*

A 2-issue patch on v1.9.0 (shipped 2026-06-11). No migration, no operator action.

- [#403](https://github.com/guycorbaz/mybibli/issues/403) — **i18n-bundle sweep.** `inline-form.js` (modal-busy guard) and `mybibli.js` (htmx:responseError feedback) drop their hand-synced `{en, fr}` string objects and read the server-rendered `#i18n-bundle` data island — both messages localize in de/it for the first time. Completes the #386 follow-up.
- [#412](https://github.com/guycorbaz/mybibli/issues/412) — **Test de-flake.** The recent-activity boundary tests raced the second hand between two `NOW()` calls (backdate UPDATE vs count SELECT); boundary rows now carry clock-skew slack. Test-only.

## v1.9.0 — saved searches + per-provider timeouts *(shipped)*

A 4-issue feature minor on v1.8.6 (shipped 2026-06-11). One new database migration (seeds the per-provider timeout rows); applies automatically — no operator action required.

- [#367](https://github.com/guycorbaz/mybibli/issues/367) — **Saved custom searches.** A home-page search composition (free-text `q` + filter chip + sort column/direction) can be saved under a name and re-run in one click from a *Saved searches* dropdown next to the sort control; entries are renamed/deleted via UX-DR8 modals from the same dropdown. Instance-wide (single-tenant model, like the settings table) — no per-user scoping. The query-builder depth stays deferred to the v2.0 classification work (#206).
- [#396](https://github.com/guycorbaz/mybibli/issues/396) — **Per-provider metadata-timeout overrides.** The v1.7.9 scalar (`metadata_chain_per_provider_timeout_secs`) becomes the fallback default; /admin > System > Metadata Providers gains one timeout field per registered provider (`provider_timeout.<slug>` K/V rows, empty = use default, 1–60 s bounds). Resolution is hot — the chain reads the override map on every fetch. Split out of #384 during the v1.8.5 robustness sweep.
- [#406](https://github.com/guycorbaz/mybibli/issues/406) — **Admin log-level form 409 fix.** `fetch_setting_rows` omitted `log_level` (and the two #334 timeout keys) from its `IN` list, so the form re-rendered with a hardcoded version 1 and every save after the first 409'd forever. The keys are now in the list and the regression class is documented in the function contract.
- [#405](https://github.com/guycorbaz/mybibli/issues/405) — **Transport-crate log hygiene.** `combine_log_directives` prepends `hyper_util=warn,reqwest=warn,hyper=warn` to the operator's directive on both the boot path and the runtime-reload path, killing ~2 MB/day of connection-pool DEBUG chatter at global `debug`; an explicit operator directive for those crates still wins.

## Older shipped releases

v1.8.6 back to v1.2.0 — the seven themed minors ("See what you own, find it faster" → "Covers, honestly") and their patch trains — are documented release-by-release on the [GitHub releases page](https://github.com/guycorbaz/mybibli/releases) and in chapter 8 of the user manual. This file only keeps the current minor and its immediate predecessors.

## v2.0.0 candidate — "Classification, reinvented"

The only currently-imagined breaking change. May land on a minor if a
backwards-compatible path is found.

- [#206](https://github.com/guycorbaz/mybibli/issues/206) — Split `genre` from auto-fetched metadata; introduce a "classification" surface combining Dewey + manual genre, with a migration that preserves existing data.

## Parking lot — no scheduled version

Work that is tracked but not slotted lives in the
[issue tracker](https://github.com/guycorbaz/mybibli/issues) under
`type:change-request`, `type:code-review-finding` and
`type:known-failure` (Foundation Rule 11 — GitHub Issues is the single
source of truth, and this file deliberately keeps no second copy of it).
Such items are picked up opportunistically when a related area is being
worked on, or rolled into a v1.x release if they happen to fit a theme.

*The parking lot is currently empty.* The eleven findings this section
used to list — #145 to #152, #202, #216, #217, #220, #389 — have all
been closed.

## Past releases

The shipped history is documented release-by-release at
[github.com/guycorbaz/mybibli/releases](https://github.com/guycorbaz/mybibli/releases)
and in chapter 8 ("Upgrade & migration") of the user manual under
`docs/manual/{en,fr}/`. The website's [roadmap page](https://guycorbaz.github.io/mybibli/roadmap.html)
also tells the story in a more digestible form.
