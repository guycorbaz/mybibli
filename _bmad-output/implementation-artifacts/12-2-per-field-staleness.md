# Story 12.2: Per-field staleness

Status: ready-for-dev

GitHub issues: #542 (type:change-request) — closes it; #535 items 1–2 (type:code-review-finding) — decided 2026-10-30, done here (items 3–5 stay open in #535). Covers: FR149 (refines FR140); resubmission rule of FR136 refined per the #535 decision. Epic 12 — second story; ships in v1.22.0.

## Story

As a librarian,
I want accepting a cover not to make the same title's metadata proposal stale, and the reverse,
so that I can review an agent's proposals in any order without having them filed again.

## Decisions (project lead, 2026-10-30)

- **#542 — per-field staleness.** A metadata or cover proposal goes stale only when **something it touches** changed since filing (or its title was deleted). A hand edit of a touched field still makes it stale — nothing is ever overwritten. Series proposals keep today's rules.
- **#535 item 1 — cover rejection blocks only the same image.** Equivalence for a cover becomes *same title + same image* (content fingerprint), not *same title*.
- **#535 item 2 — metadata rejection blocks per field.** A rejected metadata proposal blocks, for the resubmission delay, **each of its fields** wherever it reappears (overlap), not only the exact same field set.
- Context: on 2026-10-30, 55 of ~300 decisions went stale because a cover accepted on a title bumped `titles.version` (an unusual session — many freshly catalogued titles), and the truncated covers rejected that day (#558) block any cover for those titles for 90 days.

## Existing-code reality check (code map, 2026-10-30)

| Place | Today | Change |
|---|---|---|
| `src/services/proposals.rs::stale_reasons` (l. 602) | a target is stale when deleted **or** its `version` differs from the item's `target_version` | kind-aware: for `metadata` / `cover`, only **deleted** counts; series kinds unchanged |
| `ProposalService::check_staleness_in` (l. 981) and `transition_in` (l. 877, Accept branch l. 912–922) | both call `stale_reasons` — `transition_in` re-checks after the kind's own checks | both must apply the same kind-aware rule, or `transition_in` turns a per-field-fresh proposal stale |
| `metadata_acceptance::accept_in` (l. 311) | version check, then **already** a per-field check `field_changed(stored current vs live)` → `AcceptConflict::FieldChanged`, and a role check → `RoleGone` | keep these; they become the whole staleness rule for metadata |
| `cover_acceptance::accept_in` (l. 187) | version check, then **already** `cover_changed(stored current vs live cover_image_url)` → `AcceptConflict::CoverChanged` | keep; it becomes the whole rule for covers |
| `services/proposal_history.rs::ProposalSubject::equivalence_key` (l. 229) | `metadata:titles/{id}:{f1,f2}` (exact set), `cover:titles/{id}` | cover → `cover:titles/{id}:{fingerprint}`; metadata key format unchanged, the **check** becomes per-field |
| `proposal_history::check_resubmission` (l. 300) + `ProposalModel::latest_rejection` (`models/proposal.rs` l. 826) | exact-key lookup of the newest live rejection | metadata: newest live rejection among keys `metadata:titles/{id}:%` whose field set **intersects** the new one (index `idx_proposals_equivalence` serves the prefix `LIKE`); cover and series: exact key as today |
| `ProposalHistoryService::list_mine` (`resubmittable_from`, l. 360–405) + `latest_rejection_dates` | per exact key | metadata: the newest overlapping rejection; cover: the fingerprinted key |
| `cover_proposals::cover_item` (l. 332) | JSON of the cover line: `staged_file`, size, `current`… — no content fingerprint | add `"fingerprint"`: SHA-256 (hex) of the **re-encoded** JPEG (`EncodedCover::jpeg`, deterministic for the same pixels) |
| `Cargo.toml` | no hash crate; `sha2 0.10.9` already in `Cargo.lock` (transitive) | add `sha2 = "0.10"` (no new crate in the tree) |
| `src/mcp/tools/list_my_proposals.rs` description (l. 104–113) | "the same title and the same set of fields for metadata, the same title for a cover" | update the equivalence wording (agent-facing, English) |
| `CLAUDE.md` § Proposal lifecycle | "accepting fields or a cover bumps `titles.version`, which makes every other pending proposal on that title stale whatever its kind (#542, open — do not change without the user)" | rewrite: per-field rule, decided 2026-10-30 |

Data model: no migration (`equivalence_key` is `VARCHAR`, `proposal_items.proposed_values` JSON). `titles.version` still bumps on accept — only its use for metadata/cover staleness goes.

## Acceptance Criteria

1. **AC1 — cover then metadata.** Given a pending metadata proposal and a pending cover proposal on the same title, when the cover proposal is accepted, then the metadata proposal: both apply; neither is stale. Same in the reverse order.
2. **AC2 — disjoint metadata.** Given two pending metadata proposals on the same title touching disjoint fields, when both are accepted in turn, then both apply.
3. **AC3 — touched field edited.** Given a pending metadata proposal for `description`, when the description is changed (hand edit, another accepted proposal, or an automatic re-fetch) before it is accepted, then accepting it applies nothing and marks it stale with the existing `FieldChanged` copy. Same for a cover proposal when the cover changed (`CoverChanged`).
4. **AC4 — untouched field edited.** Given a pending metadata proposal for `description`, when the publisher is edited by hand meanwhile (bumping `titles.version`), then accepting it applies the description — not stale.
5. **AC5 — contributors.** Given a pending metadata proposal adding contributors, when one of them was credited in the same role meanwhile, then the existing rule applies (already-credited ones skipped), not stale; a deleted role still makes it stale (`RoleGone`).
6. **AC6 — deleted title.** A title soft-deleted since filing still makes any metadata or cover proposal on it stale (`Deleted`).
7. **AC7 — series kinds unchanged.** `series_create` / `series_attach` keep the version + live-state checks (#521) exactly; their tests pass unchanged.
8. **AC8 — `transition_in` consistent.** The Accept branch of `transition_in` applies the same kind-aware rule as `check_staleness_in` (no proposal that passed the kind's checks goes stale there).
9. **AC9 — cover equivalence by image.** A cover proposal records a `fingerprint` (SHA-256 hex of the re-encoded JPEG) in its cover line; its equivalence key is `cover:titles/{id}:{fingerprint}`. A rejected cover blocks, for the delay, only a proposal of the **same image** for the same title; a different image is filed. Covers rejected before this story (legacy key `cover:titles/{id}`, no fingerprint) block nothing new — intended: it unblocks the truncated covers of 2026-10-30.
10. **AC10 — metadata rejection per field.** A rejected metadata proposal `{publisher}` blocks, for the delay, any metadata proposal for the same title that includes `publisher` (`{publisher, publication_year}` is refused); `{publication_year}` alone is filed. The refusal message names the blocking field(s) and the open-from date (existing `RecentlyRejected` shape; English, agent-facing).
11. **AC11 — history.** `list_my_proposals` `resubmittable_from` reflects the new rules (metadata: newest live rejection overlapping the proposal's fields; cover: same image); the tool description states the new equivalence (per field for metadata, same image for a cover). The registry test stays green.
12. **AC12 — docs in code.** `CLAUDE.md` § Proposal lifecycle and the module docs of `proposals.rs` / `acceptance.rs` / `proposal_history.rs` describe the per-field rule; the manual's *Outdated proposals* and resubmission paragraphs (EN + FR, chapter 13) are updated and the PDFs rebuilt — or, if kept for Story 12.7, the story notes say so explicitly.
13. **AC13 — tests.** Unit: kind-aware `stale_reasons`, cover fingerprint and key, metadata overlap. DB integration (`tests/proposal_accept_metadata_cover.rs`, `tests/proposal_history.rs`): AC1–AC6, AC8 (accept through the real path, not only the pure functions), AC9 (same image refused, other image filed, legacy key not blocking), AC10. E2E (`metadata-cover-proposal-accept.spec.ts` or a new spec): an agent files a cover and a metadata proposal on one title; the librarian accepts the cover, then the metadata — both applied, neither outdated.

## Tasks / Subtasks

- [ ] T1 — kind-aware staleness in `proposals.rs`: `stale_reasons` gains the proposal kind (or a `StaleRule`), `check_staleness_in` and `transition_in` read the kind and pass it (AC6–AC8); unit tests
- [ ] T2 — metadata / cover acceptance: keep `field_changed` / `RoleGone` / `cover_changed` as the live checks; confirm their order with T1 (AC1–AC5); DB tests
- [ ] T3 — cover fingerprint: `sha2` dependency, fingerprint in `cover_item`, `ProposalSubject::Cover { title_id, fingerprint: Option<String> }` read from the line, key `cover:titles/{id}:{fp}` (legacy `cover:titles/{id}` when absent) (AC9); unit + DB tests
- [ ] T4 — metadata per-field resubmission: model query for live rejections with the title's key prefix inside the delay; overlap computed in Rust from the stored key's field list; `check_resubmission` and `list_mine` use it (AC10, AC11); unit + DB tests
- [ ] T5 — agent wording: `list_my_proposals` description (AC11); grep the propose tools' descriptions for the old equivalence wording
- [ ] T6 — `CLAUDE.md`, module docs, manual (AC12)
- [ ] T7 — E2E (AC13)
- [ ] T8 — gates: clippy `--all-targets -D warnings`, targeted tests (lib `services::proposals`, `services::proposal_history`, `services::*acceptance`, `services::cover_proposals`; DB `proposal_*`, `mcp_*proposals`), `cargo sqlx prepare --check` if a typed query is added; full suites once on the final code before the single push

## Dev Notes

- **Where the rule lives.** One place decides which reasons count for which kind — a `fn counts_as_stale(kind, reason)` or a parameter on `stale_reasons`. Do not special-case metadata/cover in two call sites with copied conditions (Rule #1).
- **Lock order unchanged:** proposal → items → targets (`lock_pending_in`, then `read_target_states(..., lock: true)`), then the kind's live reads (`TitleModel::find_by_id_in(.., true)`, credits, roles). The target lock still matters: it serialises a concurrent edit of the same title.
- **`target_version` is still recorded** at filing (series kinds need it; the detail page may show it); just not compared for metadata/cover.
- **Metadata `field_changed`** compares the stored per-item `current` with the live value — read it to confirm how `publication_year`, `dewey_code`, empty strings and `None` are compared; per-field staleness is exactly as good as that comparison. Add a test per field type if one is missing.
- **Edited lines (11-19):** a librarian-edited line keeps the agent's `current` (the value at filing) — the per-field check must still use the filing-time `current`, not the edited proposed value.
- **Fingerprint:** hash the bytes actually staged (`encoded.jpeg`), hex lower-case, 64 chars; `MAX_EQUIVALENCE_KEY_CHARS` must hold `cover:titles/{id}:` + 64 (the constant and the column are 700 characters — ample room).
- **Per-field overlap query:** keys look like `metadata:titles/{id}:{f1,f2,…}` (fields sorted, comma-joined, from `BTreeSet`). Query `equivalence_key LIKE 'metadata:titles/{id}:%' AND status = 'rejected' AND deleted_at IS NULL AND decided_at IS NOT NULL` (bind the prefix, escape nothing user-controlled — the id is numeric), newest first; parse the field list after the second `:` and intersect. Return the newest overlapping rejection (and which fields overlap, for the message).
- **Story 12.5 (title proposable)** will add `title` to the field list — keep the overlap generic over field names.
- **CLAUDE.md** currently says #542 must not change without the user — the user decided on 2026-10-30; update that sentence in this story.

### Regression map

- `tests/proposal_accept_metadata_cover.rs` likely asserts "accepting the cover makes the metadata proposal stale" (the #542 behaviour) — that assertion is now **inverted**; update it and say so in the completion notes.
- `tests/proposal_history.rs` asserts the cover key `cover:titles/7` and exact-set metadata equivalence — update to the new rules; keep the series cases untouched.
- `proposal_history.rs` unit test l. 597 (`cover(7) == "cover:titles/7"`) → fingerprinted form.
- E2E specs asserting an *Outdated* outcome after accepting a sibling proposal (grep `Outdated|Périmée|stale` in `tests/e2e/specs`) must be revisited.
- Smoke 11-23 accepts a series proposal only — unaffected.

### Testing standards

- Unit tests alongside code; DB tests `#[sqlx::test(migrations = "./migrations")]`; E2E `loginAs`, unique ISBNs (pick an unused specId), no `waitForTimeout`, i18n-aware regexes.
- Heavy commands under `mem-guard`; never `cargo fmt` the whole repository.

### References

- `_bmad-output/planning-artifacts/epics.md` § Epic 12, Story 12.2, FR149, FR140, FR136
- Issues #542, #535 (decision comment of 2026-10-30)
- Stories 11-9 (proposal schema, `stale_reasons`), 11-15 (equivalence, resubmission delay), 11-18 (metadata/cover acceptance, `FieldChanged` / `CoverChanged`), 11-19 (edits)
- `CLAUDE.md` § Proposal lifecycle; Foundation Rules #1, #2, #3, #6, #13

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created

### File List
