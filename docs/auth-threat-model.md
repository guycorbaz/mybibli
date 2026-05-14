# mybibli Auth Threat Model

**Status:** authoritative baseline for changes to the session, cookie, CSRF and login surfaces.
**Scope:** what the auth layer defends against, what it does not, and why the current posture is appropriate for the deployment shape.
**Audience:** maintainers reviewing PRs that touch `src/middleware/auth.rs`, `src/middleware/csrf.rs`, `src/routes/auth.rs`, the `sessions` schema, or any cookie set/read code.

This document closes the four-deferral chain captured in [#17](https://github.com/guycorbaz/mybibli/issues/17) (sub-items deferred across stories 1-2, 7-1, 7-3, and a CodeQL alert). It is referenced from `README.md` § Security and `CLAUDE.md` § Architecture.

---

## 1. Deployment shape

mybibli ships as **single-tenant**: one library instance per deployment. The reference operator is a household running it on a Synology NAS (or equivalent home server) for personal collection management, with an optional small-association mode (a literature club, a school book corner) where a handful of trusted librarians share the catalog.

| Property | Value |
|---|---|
| Tenancy | Single (one DB, one set of users, one `AppSettings` row family) |
| Network | LAN by default; optional reverse-proxy with TLS termination for external access |
| Operator count | 1 admin in 95% of installs; up to ~5 librarians in association mode |
| Trust posture | LAN clients are trusted-by-default at the network layer; the auth surface enforces the role boundary, not the network boundary |
| Stake of data | Catalog metadata + borrower contact info + loan history. Recoverable from backup. Not financially sensitive, not regulated. |

The single-tenancy is a load-bearing assumption — several design choices in CLAUDE.md § Architecture follow from it (no per-user authorization scope, MariaDB deadlock retry instead of a topology fix, one global `AppSettings`). Re-opening multi-tenancy would re-open this threat model.

---

## 2. Trust boundaries

| Boundary | Inside | Outside |
|---|---|---|
| Browser ↔ app | The authenticated session (cookie value + DB-side `sessions` row) | The page DOM (anything reachable from `document`), browser extensions, other tabs in the same browser profile |
| App ↔ DB | The mybibli process holding the `mysql://` connection pool | Anything else on the host or LAN |
| LAN ↔ Internet | The home network (router, NAS, household devices) | The internet at large, including drive-by visits to attacker-controlled sites |
| Reverse-proxy ↔ app | TLS-terminated HTTPS traffic from the operator's proxy of choice (Nginx, Caddy, Traefik) | The proxy itself is operator-managed and not part of mybibli |

CSRF protection lives at the **Browser ↔ app** boundary. The other three boundaries are operator-controlled (DB credentials, LAN security, proxy config) and are out of scope for this document.

---

## 3. Attacker models considered

### 3.1 Drive-by browser
A household member or visitor opens an attacker-controlled site while logged into mybibli in another tab. The site attempts:

- A POST to `/catalog/scan`, `/loans`, `/locations`, … to corrupt the catalog
- A POST to `/admin/users/{id}/deactivate` to lock the admin out
- A POST to `/logout` (just to annoy)

**Defense:** CSRF synchronizer token middleware (`src/middleware/csrf.rs`, story 8-2). Every state-changing method (POST, PUT, PATCH, DELETE) is required to carry a `_csrf_token` hidden form input or an `X-CSRF-Token` header matching the session's stored token. The token is minted per session row, compared via constant-time `subtle::ConstantTimeEq`, and never leaks to JS-readable surfaces (`HttpOnly` cookie holds the session ID; the CSRF token lives in a `<meta>` tag readable by same-origin scripts only).

The exempt-route allowlist is frozen at exactly `[("POST", "/login")]` and policed by `src/templates_audit.rs::csrf_exempt_routes_frozen` — adding a new route to that list requires editing both the constant and the test.

### 3.2 LAN-local opportunist
Another device on the LAN (a roommate's laptop, a smart fridge, a guest's phone) tries to access mybibli. Two sub-cases:

- **Anonymous browse**: the catalog is intentionally public to anonymous viewers (UX-DR1). No defense; this is *by design*.
- **Writes**: blocked at the role boundary. Anonymous users hit `/login` on any state-changing route (`src/middleware/auth.rs::Session` extractor → 303 + `next=…`).

If the operator does not want anonymous reads to be reachable on the LAN, they must put a reverse proxy in front and gate at the network layer. The app does not implement IP allowlisting.

### 3.3 Post-compromise lateral
An attacker gains LAN access (e.g., compromised guest device) and tries to escalate via the app:

- **Brute-force login**: rate-limited at the proxy layer if any; *no* application-side throttle. Acceptable because the attacker would need to first compromise the LAN (a much larger boundary) AND a brute-force attempt against argon2id passwords scales poorly.
- **Session theft via XSS**: mitigated by strict CSP (no `unsafe-inline`, no `unsafe-eval`, no inline scripts; see `src/middleware/csrf.rs::csp` and story 7-4) which prevents the canonical token-exfil vector. The session cookie is `HttpOnly` so even a CSP bypass via approved scripts (which would require an upstream CDN compromise — out of scope) does not expose the token to JavaScript.
- **Lateral via stolen DB credentials**: out of scope; the DB credentials are managed by the operator and the threat is "the operator was compromised", which subsumes this.

### 3.4 Out of scope

- Physical access to the NAS (operator's responsibility)
- TLS man-in-the-middle on the LAN (operator's responsibility — use HTTPS via reverse proxy if needed)
- Compromised pre-built Docker image (Docker Hub TLS + image signing; not mybibli's surface)
- Nation-state actors with a 0-day in Axum / SQLx / MariaDB (we patch when CVEs land — see `cargo audit` in CI)

---

## 4. What mybibli defends against

| Defense | Lives in | Test/audit |
|---|---|---|
| **CSRF synchronizer token** on every state-changing method | `src/middleware/csrf.rs` | `src/templates_audit.rs::forms_include_csrf_token` (every `<form method="POST">` in Askama templates carries `_csrf_token`); `csrf_exempt_routes_frozen` (allowlist is `[("POST", "/login")]` and nothing else) |
| **SameSite=Lax** session cookie | `src/middleware/auth.rs` | Unit test on cookie attributes; rationale in CLAUDE.md (downgraded from Strict in 7-3 so the language-toggle POST works) |
| **HttpOnly** session cookie | `src/middleware/auth.rs` | Same |
| **Session rotation on login** — pre-login session token is invalidated, a fresh row is issued | `src/services/auth::authenticate_session` | Unit test in `services/auth.rs` |
| **Last-active-admin guard** — refuses to deactivate the last admin or self-deactivate the current session's user | `src/services/users.rs` | Unit + DB-integration tests; defense against admin lockout via UI |
| **Scanner-guard** — captures `keydown` while a modal is open to prevent USB scanner bursts from leaking into the modal's default-focused button | `static/js/scanner-guard.js` | Documented as story-7-5 invariant; no specific unit test (browser-side) |
| **Constant-time token compare** — defeats timing-based token-recovery attempts | `src/middleware/csrf.rs` via `subtle::ConstantTimeEq` | Type system; the wrong-comparison branch wouldn't compile |
| **Strict CSP** — `script-src 'self'`, `style-src 'self'`, no `unsafe-inline` / `unsafe-eval` | `src/middleware/csrf.rs::csp` + `src/templates_audit.rs::templates_have_no_inline_scripts_styles_or_event_handlers` | Story 7-4; toggle via `CSP_REPORT_ONLY=true` for observation mode |
| **Anonymous-session purge** — `sessions` rows for anonymous visitors expire after 7 days of inactivity | `src/tasks/anonymous_session_purge.rs` | `purges_old_anonymous_rows_only` integration test |
| **CSRF token never logged** | `tracing` macros never bind the raw token | Code review |

---

## 5. What mybibli does NOT defend against (accepted posture)

### 5.1 `MYBIBLI_COOKIE_SECURE=false` by default

The session and `lang` cookies are emitted **without** the `Secure` attribute when `MYBIBLI_COOKIE_SECURE` is unset or `false`. This is intentional for the LAN HTTP dev/home-server case: a `Secure` cookie is silently rejected by the browser over plain HTTP, which would make the entire auth flow unusable on a NAS that does not have HTTPS configured.

| Deployment shape | Recommended flag value | Why |
|---|---|---|
| Local dev (`docker compose up`) | `false` (default) | localhost HTTP; `Secure` would block login |
| LAN-only NAS (HTTP) | `false` (default) | LAN HTTP; same reason; threat model accepts that LAN is trust-boundary-equivalent for this surface |
| Production behind a TLS reverse proxy | **`true`** | The proxy terminates HTTPS; the cookie should be transmitted over HTTPS-only; LAN HTTP is not a use case |

Operators running behind a TLS proxy MUST set `MYBIBLI_COOKIE_SECURE=true` in their `.env`. The flag is documented in `README.md` § Configuration and `.env.example`.

This addresses CodeQL alert #10 (`Cookie missing Secure flag`) — the alert is accepted-as-suppressed for the default LAN HTTP path; the flag handles the HTTPS path correctly. If CodeQL re-raises the alert after a code change, re-read this section before silencing or refactoring.

### 5.2 `lang` cookie shares the session cookie's posture

The `lang` cookie (set when a user toggles UI language via the nav-bar control) carries `SameSite=Lax` and follows the same `MYBIBLI_COOKIE_SECURE` toggle as the session cookie. Rationale: the cookie has zero auth/auth-z payload (just `en` or `fr`), and aligning its attributes with the session cookie's keeps the cookie-policy story uniform. Tampering with the `lang` cookie has no security impact — at worst, the attacker forces a language switch.

This addresses the story 7-3 deferral from #17.

### 5.3 No per-user authorization scope (admin = god)

Any Admin can manipulate any other Admin's account, settings, or audit trail. There is no "you can only edit users you created" scope. Rationale: single-tenant households have 1 admin in 95% of installs; multi-admin associations operate on trust ("the librarian club"). See CLAUDE.md § Single-tenant.

The only ownership-style guard is the **self-deactivate** + **last-active-admin** pair in `src/services/users.rs`, both of which protect against accidental lockout, not against malicious admin behavior.

### 5.4 No application-side login rate limit

Login throttling is delegated to the operator's reverse proxy (or absent if the proxy is also absent). Rationale: brute-forcing argon2id with `m=19456, t=2, p=1` parameters is computationally prohibitive at NAS-scale traffic; adding a token-bucket in Axum middleware would add complexity for marginal benefit and risk locking out the legitimate single user.

### 5.5 No CSRF on `POST /login`

`POST /login` is the *only* member of the `CSRF_EXEMPT_ROUTES` allowlist. The freeze is policed by `csrf_exempt_routes_frozen`. Rationale: a pre-auth user has no session row yet, hence no token to validate; the login form is the bootstrap surface for the token chain. The token is rotated immediately on successful authentication.

Adding routes to the allowlist is a hard ask — any new exempt route requires updating both the constant and the test, and ideally a comment in this document explaining why.

---

## 6. Cross-reference with issue #17 sub-items

| Sub-item (from #17) | Status | Where |
|---|---|---|
| No CSRF protection on `POST /catalog/scan` (deferred from 1-2) | ✅ **Implemented** | Story 8-2 CSRF synchronizer-token middleware covers every state-changing method, including `POST /catalog/scan`. Test: `forms_include_csrf_token` enforces the hidden input is present on the scan form. |
| `/logout` exposed as GET link (deferred from 7-1) | ✅ **Implemented** | Story 8-2: `GET /logout` returns 405; logout flows through a `POST` form with CSRF token in `nav_bar.html`. Test: `csrf_exempt_routes_frozen` ensures `/logout` is NOT in the allowlist. *(Note: the mobile nav menu logout button is a follow-up tracked as story 10-2.)* |
| Session cookie missing `Secure` flag (CodeQL #10, deferred from 7-3) | ⚠️ **Accepted posture** | See § 5.1. Toggle via `MYBIBLI_COOKIE_SECURE`. Operators running HTTPS MUST set the flag to `true`. |
| `lang` cookie `SameSite=Lax` + no `Secure` flag (deferred from 7-3) | ⚠️ **Accepted posture** | See § 5.2. Same `MYBIBLI_COOKIE_SECURE` toggle. |

---

## 7. Conventions for future PRs

A PR that touches any of the following must:

- `src/middleware/auth.rs` or `csrf.rs` — re-read § 3 and § 4 of this doc and update if the threat surface changes
- `src/services/auth.rs` — update § 4 row "Session rotation on login" if behavior shifts
- The `CSRF_EXEMPT_ROUTES` constant — update § 5.5 with the new entry's rationale
- The cookie attribute set in `auth.rs` or `csrf.rs` — update § 5.1 / § 5.2 if the default changes
- The CSP header in `csrf.rs::csp` — update § 4 "Strict CSP" row

If a PR is adding a brand-new authentication factor (TOTP, WebAuthn, OIDC), this document needs a new section 3.x covering the new attacker surface that factor opens. Don't ship the feature without the doc update.

---

## 8. Open questions / known limitations

- **Session fixation across logout/login on the same browser tab.** Cookie is rotated on login (a fresh session row + token); we have not formally tested that the *pre-login anonymous* session token is invalidated rather than just re-bound. Behavior is correct in practice but the regression test would be welcome.
- **`X-CSRF-Token` header path for fetch() calls.** `static/js/csrf.js` listens on `htmx:configRequest` and injects the token, but a hand-written `fetch()` in future JS will need the same wiring. If we add more `fetch()` call sites, factor the header injection into a small helper.
- **Anonymous-session purge race with active visitor.** The 7-day purge runs daily; an anonymous visitor whose session is exactly at the boundary could lose their CSRF token mid-action. Probability is low and the recovery path (refresh) is trivial; documented here for completeness.
