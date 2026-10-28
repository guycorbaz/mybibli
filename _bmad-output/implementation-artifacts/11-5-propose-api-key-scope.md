# Story 11.5: The `propose` API-key scope

Status: ready-for-dev

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

- [ ] T1 — Migration (AC4): `migrations/20261028000000_api_keys_scope_propose.sql` — `ALTER TABLE api_keys MODIFY COLUMN scope ENUM('read', 'write', 'propose') NOT NULL;` with a header comment citing story 11-5 / FR126 / NFR44 and the `mybibli_pr_` prefix.
- [ ] T2 — Model (AC1, AC3, AC5): `ApiKeyScope::Propose`; `as_str` → `"propose"`, `short` → `"pr"`; `from_db` explicit arms for the three values, `_` → `tracing::warn!` + `Read`; `allows_propose()` → `matches!(self, Propose)`; doc comments state the matrix (read: R; write: R+W; propose: R+P; W and P mutually exclusive).
- [ ] T3 — Middleware (AC2, AC3): `ApiKeyPropose` extractor next to `ApiKeyWrite`, sharing `resolve_api_key` / `forbidden_response` (no duplication — Rule #1); module doc lists three extractors; `ApiKeyWrite`'s warn log names the actual scope (`scope = ctx.scope.as_str()`), not "read scope".
- [ ] T4 — Admin UI (AC1, AC6): `admin_api_keys.rs` — parse `"propose"`, list chip `bg-purple-100 text-purple-800`, panel fields `scope_propose` / `scope_propose_help`; template third radio.
- [ ] T5 — Locales (AC6): `admin.api_keys.scope_propose`, `scope_propose_help`, updated `intro` + `error_invalid_scope` in en/fr/de/it; `touch src/lib.rs`; `cargo test --test locale_parity`.
- [ ] T6 — Docs: `docs/route-role-matrix.md` § HTTP API v1 — three scopes, `ApiKeyPropose` named, PATCH note "a read-only or propose key gets 403". (User-manual chapter is story 11-24's scope.)
- [ ] T7 — Unit tests (AC7): in `models/api_key.rs` — scope matrix table test, round-trip `from_db(as_str(s)) == s` for all three, `short()` values distinct, `mint_plaintext_key(Propose)` → `mybibli_pr_` + 12-char prefix, unknown → Read; update `scope_from_db_falls_back_to_read_for_unknown`.
- [ ] T8 — Integration tests (AC2–AC4, AC7): `tests/api_v1_auth.rs` — propose key GET `/api/v1/titles` 200, PATCH 403 with `scope_required = "write"` and title unchanged; new `tests/api_key_propose_scope.rs` — a test router with one `ApiKeyPropose` route through `build_router`-equivalent state: missing → 401, read → 403 `scope_required = "propose"`, write → 403, propose → 200; admin create `POST /admin/api-keys` with `scope=propose` (session + CSRF, like `tests/admin_api_keys_delete.rs`) → row stored with `scope = 'propose'`, `key_prefix` starts `mybibli_pr_`, response modal carries the plaintext; `information_schema` column type is `enum('read','write','propose')`; an invalid scope still answers 400.
- [ ] T9 — E2E (AC7): `tests/e2e/specs/journeys/admin-api-keys.spec.ts` — `loginAs(page, "admin")`, `/admin?tab=api_keys`, unique label, pick the propose radio by role, submit; the created modal shows a plaintext matching `/^mybibli_pr_[A-Za-z0-9]{40}$/`; close; the list row shows the label and the propose chip; `page.request` with `Authorization: Bearer` → GET `/api/v1/titles` 200, PATCH `/api/v1/titles/1` 403 `insufficient_scope`; revoke the key at the end via its UX-DR8 modal (leaves no active key behind). i18n-aware regexes; no `waitForTimeout`.
- [ ] T10 — Gates: clippy `--all-targets -D warnings`, full `cargo test`, `cargo sqlx prepare --check` (dynamic `sqlx::query` only — no `.sqlx` change expected), `tsc --noEmit`, full E2E at `--workers=2`.

## Dev Notes

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

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-10-28 — story created (create-story).
- 2026-10-28 — validated against the create-story checklist: extractor order, consumer grep and manual-scope notes added.
