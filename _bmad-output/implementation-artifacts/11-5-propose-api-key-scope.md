# Story 11.5: The `propose` API-key scope

Status: done

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR126, NFR44. Epic 11 — fifth story; prerequisite of 11-6 (the MCP endpoint accepts only `propose` keys).

## Story

As an admin,
I want to issue an API key that can only read and propose,
so that an AI agent can never modify the catalog directly.

## Existing-code reality check (code map, 2026-10-28)

| Place | Today | Consequence for this story |
|---|---|---|
| `migrations/20260519141801_create_api_keys_table.sql` | `scope ENUM('read', 'write') NOT NULL` | MariaDB strict mode refuses `'propose'` → a new migration widens the ENUM (append the value; existing rows keep their stored value) |
| `src/models/api_key.rs` `ApiKeyScope` | two variants; `as_str` / `short` (`ro` / `rw`); `from_db` maps `"write"` → Write, **anything else → Read silently**; `allows_read` = true, `allows_write` = Write only | add `Propose` (`"propose"` / `"pr"`), explicit `from_db` arm, a new `allows_propose` (Propose only); exhaustive matches elsewhere will fail to compile until updated (that is the point) |
| `mint_plaintext_key` | `mybibli_<short>_<random40>`, prefix = first 12 chars (`mybibli_ro_X`) | `mybibli_pr_` is also 11 chars → prefix shape and length unchanged; no column change |
| `src/middleware/api_key.rs` | `ApiKeyAuth` (any resolved key) and `ApiKeyWrite` (`allows_write`, else 403 `{error: forbidden, reason: insufficient_scope, scope_required: write}`) | `ApiKeyAuth` lets a propose key read (AC2); `ApiKeyWrite` already refuses it (AC2); add `ApiKeyPropose` (`allows_propose`, else 403 `scope_required: propose`) — no route uses it yet (11-6 will) |
| `src/routes/api_v1.rs` | 6 GET handlers on `ApiKeyAuth`, `patch_title` on `ApiKeyWrite` | no change |
| `src/routes/admin_api_keys.rs` | form parses `"read"`/`"write"` (else 400 `error_invalid_scope`); list chip `match k.scope` with two arms | third arm in both places; panel struct gains `scope_propose` + `scope_propose_help` |
| `templates/fragments/admin_api_keys_panel.html` | two radios in a `<fieldset>` | third radio `value="propose"`, same markup |
| `locales/{en,fr,de,it}.yml` § `admin.api_keys` | `scope_read*`, `scope_write*`, `intro` and `error_invalid_scope` name two scopes | add `scope_propose` / `scope_propose_help`; update `intro` and `error_invalid_scope` in all four files |
| `docs/route-role-matrix.md` § HTTP API v1 | says "the scope (read / write)" and names the two extractors | mention the third scope and `ApiKeyPropose` |
| E2E | no spec drives Admin → API keys creation today | new `tests/e2e/specs/journeys/admin-api-keys.spec.ts` |

## Acceptance Criteria

1. **AC1 — create.** Given Admin → API keys, when I create a key, then I can choose the `propose` scope besides `read` and `write`, and the key gets a distinct plaintext prefix `mybibli_pr_…`. The list shows the key with its own scope chip.
2. **AC2 — read yes, write no.** Given a `propose` key, when it calls a `/api/v1` read endpoint, then the call succeeds; and `PATCH /api/v1/titles/{id}` answers 403 with `reason: insufficient_scope` (and `scope_required: write`).
3. **AC3 — exclusive right (NFR44).** A `read` or `write` key never carries the `propose` right; the new `ApiKeyPropose` extractor requires exactly the `propose` scope (403 `insufficient_scope`, `scope_required: propose` otherwise; 401 without a valid key).
4. **AC4 — migration.** A migration extends `api_keys.scope` to accept `propose`; existing keys keep their scope.
5. **AC5 — explicit mapping.** `ApiKeyScope::from_db` maps `propose` explicitly. An unknown value still falls back to `Read` (least privilege) but now logs a warning instead of being silent (D-11-5-2).
6. **AC6 — i18n.** The scope label and help text exist in all four locale files (`tests/locale_parity.rs` green); the panel intro and the invalid-scope error name the three scopes.
7. **AC7 — tests.** Unit tests cover the full scope matrix (3 scopes × read / write / propose rights, `as_str` / `short` / `from_db` round-trip, mint prefix); integration tests cover the propose key on GET / PATCH, the `ApiKeyPropose` extractor matrix, the admin create handler with `scope=propose`, and the migrated column type; E2E covers creating a `propose` key from Admin → API keys and using it (GET 200, PATCH 403).

## Tasks / Subtasks

- [x] T1 — Migration (AC4): `migrations/20261028000000_api_keys_scope_propose.sql` — `ALTER TABLE api_keys MODIFY COLUMN scope ENUM('read', 'write', 'propose') NOT NULL;` with a header comment citing story 11-5 / FR126 / NFR44 and the `mybibli_pr_` prefix.
- [x] T2 — Model (AC1, AC3, AC5): `ApiKeyScope::Propose`; `as_str` → `"propose"`, `short` → `"pr"`; `from_db` explicit arms for the three values, `_` → `tracing::warn!` + `Read`; `allows_propose()` → `matches!(self, Propose)`; doc comments state the matrix (read: R; write: R+W; propose: R+P; W and P mutually exclusive).
- [x] T3 — Middleware (AC2, AC3): `ApiKeyPropose` extractor next to `ApiKeyWrite`, sharing `resolve_api_key` / `forbidden_response` (no duplication — Rule #1); module doc lists three extractors; `ApiKeyWrite`'s warn log names the actual scope (`scope = ctx.scope.as_str()`), not "read scope".
- [x] T4 — Admin UI (AC1, AC6): `admin_api_keys.rs` — parse `"propose"`, list chip `bg-purple-100 text-purple-800`, panel fields `scope_propose` / `scope_propose_help`; template third radio.
- [x] T5 — Locales (AC6): `admin.api_keys.scope_propose`, `scope_propose_help`, updated `intro` + `error_invalid_scope` in en/fr/de/it; `touch src/lib.rs`; `cargo test --test locale_parity`.
- [x] T6 — Docs: `docs/route-role-matrix.md` § HTTP API v1 — three scopes, `ApiKeyPropose` named, PATCH note "a read-only or propose key gets 403". (User-manual chapter is story 11-24's scope.)
- [x] T7 — Unit tests (AC7): in `models/api_key.rs` — scope matrix table test, round-trip `from_db(as_str(s)) == s` for all three, `short()` values distinct, `mint_plaintext_key(Propose)` → `mybibli_pr_` + 12-char prefix, unknown → Read; update `scope_from_db_falls_back_to_read_for_unknown`.
- [x] T8 — Integration tests (AC2–AC4, AC7): `tests/api_v1_auth.rs` — propose key GET `/api/v1/titles` 200, PATCH 403 with `scope_required = "write"` and title unchanged; new `tests/api_key_propose_scope.rs` — a test router with one `ApiKeyPropose` route through `build_router`-equivalent state: missing → 401, read → 403 `scope_required = "propose"`, write → 403, propose → 200; admin create `POST /admin/api-keys` with `scope=propose` (session + CSRF, like `tests/admin_api_keys_delete.rs`) → row stored with `scope = 'propose'`, `key_prefix` starts `mybibli_pr_`, response modal carries the plaintext; `information_schema` column type is `enum('read','write','propose')`; an invalid scope still answers 400.
- [x] T9 — E2E (AC7): `tests/e2e/specs/journeys/admin-api-keys.spec.ts` — `loginAs(page, "admin")`, `/admin?tab=api_keys`, unique label, pick the propose radio by role, submit; the created modal shows a plaintext matching `/^mybibli_pr_[A-Za-z0-9]{40}$/`; close; the list row shows the label and the propose chip; `page.request` with `Authorization: Bearer` → GET `/api/v1/titles` 200, PATCH `/api/v1/titles/1` 403 `insufficient_scope`; revoke the key at the end via its UX-DR8 modal (leaves no active key behind). i18n-aware regexes; no `waitForTimeout`.
- [x] T10 — Gates: clippy `--all-targets -D warnings`, full `cargo test`, `cargo sqlx prepare --check` (dynamic `sqlx::query` only — no `.sqlx` change expected), `tsc --noEmit`, full E2E at `--workers=2`.

### Review Findings

Round 1 (2026-10-28, Blind Hunter + Edge Case Hunter + Acceptance Auditor, full mode): 0 High, 4 Medium, ~20 Low.

- [x] [Review][Patch] (Medium) Integration test passed on the list's 12-char prefix, not the modal plaintext — now matches `data-api-key-plaintext>mybibli_<short>_[A-Za-z0-9]{40}</div>` [tests/api_key_propose_scope.rs]
- [x] [Review][Patch] (Medium) E2E PATCH targeted shared title id 1 — now a non-existent id (2147483647): 403 expected, a regressed guard answers 404 and touches nothing [admin-api-keys.spec.ts]
- [x] [Review][Patch] (Medium) E2E revoke assertion ambiguous in German ("Widerrufen" is both the status and the Revoke button) and could race the commit — now waits for the revoke POST (200), the dialog hidden and the row's Revoke button gone [admin-api-keys.spec.ts]
- [x] [Review][Patch] (Medium→Low after check) Propose chip relies on a class pair used incidentally by another template — chip classes moved to `scope_chip_class()`, and `scope_chip_classes_are_emitted_by_tailwind` pins every class against `templates/` [src/routes/admin_api_keys.rs]
- [x] [Review][Patch] (Low) Scope string mapping duplicated in the form handler — `ApiKeyScope::parse` (strict) shared by the form and `from_db` [src/models/api_key.rs]
- [x] [Review][Patch] (Low) Chips had no dark-mode variants — all three scope chips carry `dark:` variants [src/routes/admin_api_keys.rs]
- [x] [Review][Patch] (Low) `ApiKeyPropose` matrix lacked a forged same-prefix key and a soft-deleted key — both added (401) [tests/api_key_propose_scope.rs]
- [x] [Review][Patch] (Low) Panel label assertion accepted EN/FR only — now any of the four locales [tests/api_key_propose_scope.rs]
- [x] [Review][Patch] (Low) E2E read check used `/api/v1/genres` only — now `/api/v1/titles` and `/api/v1/genres` [admin-api-keys.spec.ts]
- [x] [Review][Defer] (Low) Row mapping swallows `scope` decode errors via `unwrap_or_default` — deferred, pre-existing → #511
- Dismissed (verified): migration drops a DEFAULT/COMMENT (the original column had neither); other `!= Read` scope checks granting write (none — `ApiKeyScope` is matched only in the three touched files); UI advertises proposals before `/api/mcp` exists (same release, D-11-5-4); `from_db` warn flood (ENUM column, unreachable in practice); `state_with_pool` duplication (existing per-file pattern of every integration test); `?tab=api_keys` (valid tab name); write/propose mutual exclusivity (NFR44 by design); test placement in a new file instead of `api_v1_auth.rs` and the column-type test in the model module (same coverage); no pre-migration row test (ENUM append preserves values by construction).

Round 2 (2026-10-28, single combined pass): 1 Medium, 1 Low.

- [x] [Review][Patch] (Medium) Round-1 dark variants `dark:bg-sky-900` / `dark:bg-amber-900` exist in templates only with an opacity suffix, so Tailwind never emits them (light background + light text in dark mode); the emission test matched substrings — read/write chips now use `dark:bg-sky-900/40` / `dark:bg-amber-900/30`, and the test compares exact tokens (split on whitespace, quotes and template delimiters), with a self-test pinning the substring trap [src/routes/admin_api_keys.rs]
- [x] [Review][Patch] (Low) E2E "Revoke button gone" would pass if the row vanished — now asserts the row is still listed and shows its Delete button [admin-api-keys.spec.ts]


- **Least privilege by construction.** Never express propose as "write minus something": `allows_write` stays `matches!(Write)`, `allows_propose` is `matches!(Propose)`. A future scope added to the enum is then refused by both until someone opts it in.
- **Why `ApiKeyPropose` exists without a route.** 11-6 mounts `/api/mcp` behind it. The extractor is `pub`, so no dead-code lint; the integration test mounts it on a throw-away route to pin the matrix now.
- **Prefix collision.** Old keys carry `mybibli_ro_` / `mybibli_rw_` prefixes; a propose key's `mybibli_pr_` prefix can never collide with them, so the candidate scan stays unchanged.
- **Error envelope.** Reuse `forbidden_response("insufficient_scope", "propose")` — same JSON shape the docs already describe for `write`.
- **Audit.** `admin_api_keys_revoke` / `delete` already store `scope.as_str()` in `admin_audit.details` → `"propose"` appears automatically.
- **CSP / modals.** The panel radio is plain markup; no inline style, no `hx-confirm`. The created/revoke modals are unchanged.
- **Chip colour.** `bg-purple-100 text-purple-800` — distinct from read (sky) and write (amber). The Dockerfile css stage copies only `templates/` into the Tailwind scan, so a chip class written only in a `.rs` file is never emitted; purple-100/800 already appear in a template (the existing sky/amber chips survive the same way). Do not pick a colour absent from `templates/` (D-11-5-3).
- **Extractor order on PATCH.** `patch_title(State, ApiKeyWrite, Path, Json)` — Axum runs extractors in order and stops at the first rejection, so a propose key gets 403 before `Path`/`Json` are parsed; the E2E PATCH may target any id with a minimal body.
- **No other scope consumers.** Grep (2026-10-28): `ApiKeyScope` is matched only in `models/api_key.rs`, `middleware/api_key.rs` and `routes/admin_api_keys.rs`; `admin_audit` details store `as_str()`; no seed migration inserts api keys.
- **User manual out of scope.** `docs/manual/{en,fr}/11-api-integrations.tex` still describes two scopes; story 11.24 owns the `propose`-key documentation (its AC names "creating a `propose` key"). The developer-facing `docs/route-role-matrix.md` is updated here.
- **sprint-status.** Only the `11-5-propose-api-key-scope` line + line-2 header (Rule 16).

### Regression map

- `tests/api_v1_auth.rs`, `tests/api_v1_patch_titles.rs`, `tests/admin_api_keys_delete.rs` — must stay green unchanged.
- `src/models/api_key.rs::scope_from_db_falls_back_to_read_for_unknown` — semantics kept (fallback Read), extended to `propose`.
- `templates_audit` (CSRF on forms, no inline style) — the panel form already carries `_csrf_token`.

### Testing standards

Unit tests next to code (`#[cfg(test)]`), DB tests with `#[sqlx::test(migrations = "./migrations")]`, E2E with `loginAs` and stable selectors (role → id → i18n regex). Heavy commands under `mem-guard`.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.5] — ACs; FR126, NFR44 in the epic-11 header.
- [Source: src/middleware/api_key.rs] — extractor pattern.
- [Source: src/routes/admin_api_keys.rs] — create handler and list rendering.
- [Source: docs/route-role-matrix.md#HTTP API v1].
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-5-*.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- First run of `tests/api_key_propose_scope.rs`: 2 failures in the tests themselves — a freshly inserted title carries `version = 1` (not 0), and the panel renders in the configured default language; both assertions corrected (version read before PATCH, label matched in EN or FR).

### Completion Notes List

- `ApiKeyScope::Propose` (`"propose"` / `"pr"`), explicit `from_db`, `allows_propose`; rights are granted by one `matches!` each, so write and propose stay mutually exclusive (NFR44).
- `ApiKeyPropose` extractor (403 `insufficient_scope` / `scope_required: propose`); `ApiKeyWrite`'s refusal log now names the actual scope.
- Migration widens the ENUM; existing rows keep their value.
- Admin → API keys: third radio, purple chip (D-11-5-3), four locales (labels, help, intro, invalid-scope error).
- `docs/route-role-matrix.md` updated; user-manual chapter left to 11-24.
- Tests: 6 new model unit/DB tests (matrix, round-trip, prefix, ENUM column type, propose round-trip), 1 middleware envelope test, 9 integration tests in `tests/api_key_propose_scope.rs`, 1 E2E journey `admin-api-keys.spec.ts`.

### File List

- migrations/20261028000000_api_keys_scope_propose.sql (new)
- src/models/api_key.rs
- src/middleware/api_key.rs
- src/routes/admin_api_keys.rs
- templates/fragments/admin_api_keys_panel.html
- locales/en.yml, locales/fr.yml, locales/de.yml, locales/it.yml
- docs/route-role-matrix.md
- tests/api_key_propose_scope.rs (new)
- tests/admin_api_keys_delete.rs (comment only)
- tests/e2e/specs/journeys/admin-api-keys.spec.ts (new)
- _bmad-output/implementation-artifacts/11-5-propose-api-key-scope.md, epic-11-autonomous-decisions.md, sprint-status.yaml

## Change Log

- 2026-10-28 — story created (create-story).
- 2026-10-28 — validated against the create-story checklist: extractor order, consumer grep and manual-scope notes added.
- 2026-10-28 — implemented (dev-story); status → review.
- 2026-10-28 — code review round 1: 9 patches applied, 1 deferred (#511).
- 2026-10-28 — code review round 2: 1 Medium + 1 Low patched.
- 2026-10-28 — code review round 3: clean; status → done.
