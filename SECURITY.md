# Security Policy

`mybibli` is a self-hosted, multi-user web application. A defect in it does not stay
with its author: it reaches whoever runs the image on their own hardware. This document
says where to send such a defect, what happens next, and what this project does *not*
claim to defend against.

## Supported versions

| Version | Supported |
| --- | --- |
| Latest release | ✅ Security fixes land in the next release |
| Any earlier release | ❌ Upgrade to the latest tag |
| `v1.0.x` (pre-`v1.1.0`) | ❌ **Do not run.** Removed from Docker Hub |

Only the most recent release is supported. This is a one-person project shipping at a
brisk pace — there is no backport branch, and there will not be one. Upgrading is
designed to be cheap: schema migrations are purely additive, data backfills are
idempotent, and skipping intermediate tags is supported. See the
[README](README.md#installation-notes).

**`v1.0.0` … `v1.0.5` are unsafe by construction.** They shipped seed migrations that
created default `admin/admin` and `librarian/librarian` credentials on every fresh
install ([#173](https://github.com/guycorbaz/mybibli/issues/173)). Those tags have been
removed from Docker Hub. `v1.1.0` is the install floor.

## Reporting a vulnerability

**Use GitHub's private vulnerability reporting** — it is enabled on this repository:

> [**Report a vulnerability**](https://github.com/guycorbaz/mybibli/security/advisories/new)
> (repository **Security** tab → *Advisories* → *Report a vulnerability*)

The report stays private between you and the maintainer until a fix is published.

**Please do not open a public issue for a security defect.** The issue tracker is
public and indexed; a report filed there is a disclosure, not a report. If you have
already opened one, say so in the private advisory and it will be handled from there.

A useful report contains:

- the version affected (`vX.Y.Z`, or the commit SHA logged at startup);
- the deployment shape — reverse proxy or not, HTTP or HTTPS, `MYBIBLI_COOKIE_SECURE` value;
- what an attacker gains, and what position they need to start from (anonymous visitor, authenticated Librarian, LAN neighbour, …);
- steps to reproduce, ideally against the E2E stack in `tests/e2e/docker-compose.test.yml`.

The last point matters more than a severity score. This project's trust boundaries are
written down in [`docs/auth-threat-model.md`](docs/auth-threat-model.md); a report that
names the boundary it crosses is triaged in minutes rather than days.

## What to expect

This is a personal project maintained by one person in his spare time, published under
AGPL-3.0-or-later with no warranty and no service commitment. What follows is an
intention, not a contract:

| Step | Target |
| --- | --- |
| Acknowledgement of your report | within 7 days |
| Initial assessment — in scope, severity, plan | within 30 days |
| Fix released, advisory published | as soon as the fix is written and tested |

There is no bug bounty and no payment of any kind. Credit in the advisory and the
release notes is offered by default; say so if you would rather stay anonymous.

If a report goes unanswered past those windows, the maintainer is unavailable rather
than unwilling. You are free to disclose publicly after a reasonable wait — please give
the fix a chance first, since every user of this software self-hosts it and patches by
hand.

## Scope

**In scope** — the application and what ships with it:

- the Rust/Axum server, its routes, session handling, CSRF middleware and role checks;
- SQL construction and the migration runner;
- the published Docker image and the `docker-compose.yml` in this repository;
- the metadata provider chain, including how third-party responses are parsed and rendered;
- template rendering and the Content Security Policy.

**Out of scope** — the operator's responsibility, per
[`docs/auth-threat-model.md` §3.4](docs/auth-threat-model.md):

- physical access to the machine running the container;
- TLS interception on the local network — put a reverse proxy in front if you need HTTPS;
- a compromised pre-built image obtained from somewhere other than Docker Hub;
- vulnerabilities in Axum, SQLx, MariaDB or other dependencies — report those upstream. We track CVEs through `cargo audit` in CI and patch when they land.

**Documented and accepted posture — not vulnerabilities.**
[`docs/auth-threat-model.md` §5](docs/auth-threat-model.md) lists the deliberate
trade-offs of a single-household, LAN-first application. Reporting one of them as a
finding is welcome only if you can show the reasoning is *wrong* — the trade-off itself
is known:

- `MYBIBLI_COOKIE_SECURE=false` by default (§5.1), so that auth works on a NAS without HTTPS;
- no per-user authorization scope — any Admin can act on any other Admin (§5.3);
- no application-side login rate limit; argon2id parameters carry that load (§5.4);
- a persistent `Max-Age` session cookie bounded by the inactivity timeout (§5.5);
- `POST /login` exempt from CSRF, as the bootstrap of the token chain (§5.6);
- `MYBIBLI_RESET_ADMIN` writing the recovery password to the log, by design (§5.7).

## Deploying this safely

mybibli is built for a single household on a local network. Two expectations follow,
and both are the operator's to meet:

1. **Do not expose it directly to the internet.** If you must reach it from outside, put it behind a reverse proxy with TLS and set `MYBIBLI_COOKIE_SECURE=true`.
2. **Complete the first-launch wizard before adding data**, and never set `MYBIBLI_SEED_DEV_USERS=1` in production — it exists for the development and E2E stacks.

## Disclosure

Fixes are released as a normal version, with a GitHub Security Advisory and a note in
the release. Where an upgrade alone is not enough — a credential to rotate, a setting to
change — the advisory says so in its first paragraph.
