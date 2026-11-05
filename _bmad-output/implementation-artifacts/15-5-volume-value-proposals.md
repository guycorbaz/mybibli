# Story 15.5: Volume value proposals

Status: ready-for-dev

GitHub issue: #564 part 1 (type:change-request). Covers: FR165, UX-DR41 (value part). Epic 15; ships in v1.25.0.

## Story

As a librarian estimating my collection,
I want the agent to propose what my volumes are worth, with its sources,
so that the estimate covers the collection without typing each value.

## Decisions (project lead, 2026-11-04 — #564)

#564 = the base: a **current value** proposal (amount in the default currency, sources, date) plus the **list price at publication** proposed as a labelled stand-in when the volume has no purchase price; coverage on `/stats/value` is story 15-6. Estimate history, ranges and series premium come later. No FX conversion (as CR #243).

## Design decisions (dev, 2026-11-05 — recorded for the review)

- **D1 — "accepted amounts count as hand-set" is vacuous for volumes.** `manually_edited_fields` exists on `titles` only, and nothing automatic (provider, re-download, repair) writes `volumes.purchase_price` / `current_value`. No column is added (same reading as 15-4 D2); the CLAUDE.md entry says so.
- **D2 — amounts are integer cents in the item JSON** (`amount_cents`), never floats; acceptance binds a decimal string (`"18.50"`) to the `DECIMAL(10,2)` columns. Accepted range 1..=9 999 999 999 cents (the column's max). The agent's amount (a JSON number) must have at most two decimals — more is **refused**, never rounded (Rule #20).
- **D3 — currency is stated and must equal `settings.default_currency`**; another currency is refused with the default named (no conversion). Acceptance writes the line's currency; a default changed meanwhile does not outdate the proposal (the amount was sourced in that currency).
- **D4 — one pending value proposal per volume** (target index, as `pending_targeting`); the refusal names its id. Labels on the same volume do not block (different kind).
- **D5 — equivalence = the exact amounts.** `volume_value:volumes/<id>:<cents><CUR>[:list:<cents>]`; a rejected estimate blocks the same amounts on the same volume for the resubmission delay, a new estimate passes. Kept from the filing (lines are not editable, D6).
- **D6 — lines are not editable; the list-price line is removable.** The librarian accepts, rejects, or removes the list-price line (the current-value line is the last line kept). Typing another amount is the volume page's hand edit. Keeps `proposal_edit.rs` (1780 lines) and `proposal_review.rs` (1766) to dispatch arms only.
- **D7 — staleness per field (#542 convention), `version` not used** (`version_counts` → false): stale when the volume is deleted, when its `current_value` or `current_value_currency` differs from the item's `current` snapshot, or — for a list-price line — when a purchase price was set meanwhile.
- **D8 — read path in `get_title`.** `volumes[]` of the MCP document gains `edition_comment`, `purchase_price` and `current_value` (`{amount, currency}` or `null`; current value also `updated_at`, ISO 8601 UTC), and the document gains `default_currency`. Filled for MCP only (absent from `/api/v1`, as `labels`). `condition_name` is already there.

## Starting point (code map, 2026-11-05)

- Columns: `migrations/20260520100000_volumes_collection_valuation.sql` (`purchase_price`/`purchase_currency`/`current_value`/`current_value_currency` DECIMAL(10,2) / CHAR(3), `current_value_updated_at` DATETIME). Hand writer: `routes/catalog.rs::update_volume` → `VolumeModel::update_value` (pool, version-guarded, audit row `volume_value_update`).
- Volume-targeted proposals: story 13-9 (`ProposalTargetTable::Volumes`, `services/label_proposals.rs`, `label_acceptance.rs`, tool `propose_label`, `routes/proposals.rs:98` view link).
- Kind touchpoints: same list as 15-4 (`models/proposal.rs` kind + `ALL` + `subject_lines`, `proposals.rs::version_counts`, `acceptance.rs` dispatch + `Applied` + `AcceptConflict`, `proposal_accept.rs`, `proposal_history.rs` subject/key/describe, `proposal_review.rs` kind label/target/parse_line, `proposal_edit.rs` editable/keep/removable, `routes/proposals.rs`), new MODIFY migration for `proposals.kind`.
- Agent document: `services/title_detail.rs::find_agent_title_detail` (`TitleDetailVolume.labels` pattern for MCP-only fields).

## Acceptance Criteria

1. **AC1 — read path (D8).** `get_title` volumes carry `edition_comment`, `purchase_price`, `current_value` (with `updated_at`) and the document `default_currency`; the tool description says so and points to `propose_volume_value`. `/api/v1` unchanged.
2. **AC2 — kind and shapes.** Kind `volume_value` (enum + migration). Items target `volumes/<id>` with the volume `version` (recorded, unused): a **current-value line** `{"line":"current_value","amount_cents","currency","current":{"amount_cents"|null,"currency"|null}}` always; a **list-price line** `{"line":"list_price","amount_cents","currency"}` only when the volume has no purchase price. Shapes documented in the module doc.
3. **AC3 — MCP tool `propose_volume_value`** (closed schema): `volume_id`, `current_value: {amount, currency}`, `list_price?: {amount, currency}`, `note?` (≤ 500 chars, plain text, the agent's reasoning: condition, edition), `sources` (required). Propose-tool rules (description contains "does not change the catalog" and `` `sources` ``); registered in `registry()`, `PROPOSE_TOOLS`, `tests/common/mcp.rs`, `tests/e2e/helpers/mcp.ts`; `docs/route-role-matrix.md`.
4. **AC4 — filing refusals** (English, `Refusal` classes, nothing written): unknown / deleted volume; amount ≤ 0, above the max or with more than two decimals; currency ≠ default (names it); a `list_price` while the volume has a purchase price; a proposal identical to the live state (current value equal and no list price); a pending value proposal on the volume (D4); an equivalent rejection (D5); invalid sources.
5. **AC5 — acceptance.** Order: proposal → items → **volume FOR UPDATE** → staleness (D7: deleted, `ValueChanged`, `PurchasePriceSet` — new conflicts) → `transition_in` → write in the same transaction: `current_value`, `current_value_currency`, `current_value_updated_at = NOW()`, and for a list-price line `purchase_price` + `purchase_currency`; bumps `volumes.version`. Deadlock retry (`in_retried_transaction`). Audit row `volume_value_update` as the hand edit (best effort after commit). Applied feedback names the amounts.
6. **AC6 — review (UX-DR41).** The detail shows the volume (V-code, title link, condition, edition note), a current-vs-proposed table (current value with its date; purchase price), the list-price line labelled "List price at publication — stands in for the purchase price", the note and the sources. Kind label / filter `proposals.kind_volume_value`; accept modal sentence; stale reasons; all keys in **en/fr/de/it**.
7. **AC7 — history.** Subject / `describe` / `list_my_proposals` show the kind; rejection blocks the exact amounts (D5).
8. **AC8 — lock scope (epic 12 retro A1).** Locking reads start from the target index or PK; regression test `a_value_acceptance_on_one_volume_does_not_lock_proposals_of_another`.
9. **AC9 — tests.** Unit: amount parsing (decimals, range), shapes, key. DB integration (`tests/proposal_volume_value.rs`): each refusal writes nothing; accept writes the amounts and date; stale paths; list-price line removal; rollback; CSRF + role; lock scope. E2E (`tests/e2e/specs/journeys/volume-value-proposal.spec.ts`, own spec id): the agent reads a title over MCP, proposes a value and a list price, the librarian (real `loginAs`) reviews and accepts, the volume page shows the amounts. Full suites green; flake gate.
10. **AC10 — docs.** CLAUDE.md "Proposal lifecycle" gains a `volume_value` paragraph. Manual chapters are story 15-8.

## Tasks

- [ ] Migration + `ProposalKind::VolumeValue`; `cargo sqlx prepare`.
- [ ] `get_title` read path (D8).
- [ ] `services/volume_value_proposals.rs` (amounts, validation, errors + `Refusal`, filing).
- [ ] MCP tool + registries + test helpers + docs list.
- [ ] `services/volume_value_acceptance.rs` + dispatch + conflicts + copy.
- [ ] Review / edit / history arms.
- [ ] Locales en/fr/de/it.
- [ ] Tests (unit, DB, lock scope, E2E).
- [ ] CLAUDE.md.

## Dev Agent Record

### Completion Notes

### File List
