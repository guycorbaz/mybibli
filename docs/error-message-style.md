# Error-message style guide

> Status: v1 (issue #10). The one-page contract every `error.*` i18n key is
> reviewed against. Lives next to the code so retrospectives can cite it
> (Foundation Rule #9).

mybibli has **two** kinds of error message. Applying the wrong shape to a
message is itself a defect — a field-validation hint should never be a
paragraph, and an operation failure should never be a cryptic fragment.

## 1. Field-validation messages — **terse**

Shown inline next to a form field the user is actively editing. The user
already has the context (they're looking at the field), so the message states
**only what is wrong**, ideally with the constraint.

- ✅ `Username must be at least 3 characters`
- ✅ `Threshold must be ≤ 365 days`
- ✅ `Genre is required`
- ❌ `Your username could not be accepted because it does not meet the minimum length requirement of three characters. Please enter a longer username and try again.` (over-verbose for a field hint)

Rules:
- One sentence, no terminal "what to do" (the fix is obvious: fix the field).
- State the bound/format when there is one (`at least 3`, `≤ 365`, `13 digits`).
- No HTTP codes, no internal identifiers, no jargon.

## 2. Operation / conflict / system errors — **tripartite**

Shown after an action fails for a reason the user can't infer from a single
field: a conflict, a missing target, a permission gap, a backend failure.
Follow **What happened → Why → What you can do**, in plain language.

- ✅ `Another user just modified this title. Reload the page so you see the latest version, then try again.` (what → why-ish → what to do)
- ✅ `Cannot delete Hergé. This contributor is associated with 12 title(s). Remove the contributor from all titles first.`
- ✅ `Your action was rejected because the security token attached to your session has expired. This typically happens after a tab has been left open for several days. Click Reload to refresh your session and try again.`
- ❌ `Invalid request` (no what, no why, no what-to-do)
- ❌ `Cannot delete: item is referenced by active records.` (jargon: "referenced by active records"; no guidance)

Rules:
- Lead with what happened in user terms ("We couldn't create the title", not "creation_failed").
- Give the actionable next step ("Reload and try again", "Remove the links first", "Contact your administrator if it persists").
- Never expose: HTTP status codes, stack traces, SQL/SQLSTATE, internal table/column names, the word "resource".
- Security-sensitive failures (auth, internal 500) stay deliberately vague about the cause but still tell the user what to do.

## Tone

Match the rest of the UI: calm, plain, second-person ("you"), no blame, no
exclamation marks. Same register in all four locales (de / en / fr / it).

## Mechanics

- Every user-visible string is an `error.*` key in `locales/{de,en,fr,it}.yml`
  (never a hardcoded literal), rendered through `AppError::IntoResponse` +
  the FeedbackEntry fragment. The `locale_parity` test enforces 4-locale
  parity; run `touch src/lib.rs && cargo build` after adding keys.
- Field-validation vs operation is a judgement call on **where the message
  surfaces**, not on the `AppError` variant alone. A `BadRequest` used as a
  field hint stays terse; a `BadRequest` returned from a multi-step action
  gets the tripartite treatment.

## Review hook (Foundation Rule #9)

At each epic retrospective, sample the `error.*` keys touched that epic and
check them against this guide. Cryptic or jargon-laden operation errors are
defects to refactor; over-verbose field hints are defects too. Record outliers
as follow-up work rather than fixing silently mid-retro.
