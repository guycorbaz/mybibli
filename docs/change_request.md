# Change Requests — mybibli

This file tracks all change requests to the product requirements, architecture, or implementation.
Use this log to capture changes that arise after the PRD is finalized.

## Format

Each entry follows this structure:

```
### CR-XXXX: [Short title]
- **Date:** YYYY-MM-DD
- **Status:** Open | In Review | Accepted | Rejected | Implemented
- **Requested by:** [Name or role]
- **Impacts:** [PRD | Architecture | Data Model | API | UI | Tests]
- **Milestone:** [M1-M6 | Post-MVP | N/A]
- **Description:** [What needs to change and why]
- **Decision:** [Outcome and rationale, filled when resolved]
```

## Change Requests

### CR-0001: Add Undo for recent scan actions
- **Date:** 2026-03-27
- **Status:** Open
- **Requested by:** UX Design session (Party Mode)
- **Impacts:** PRD (new FR), UI
- **Milestone:** Post-MVP (Growth)
- **Description:** Librarian can undo the last scan action (detach volume, cancel location assignment) from the feedback list within a 30-second window. Acts as safety net for accidental associations. Not a trust pillar — trust comes from prevention and clear feedback — but a nice-to-have for reducing anxiety.
- **Decision:** Deferred to Growth. MVP relies on preventive validation and explicit confirmation.

### CR-0002: Cataloged error messages in i18n (human-written, not generated)
- **Date:** 2026-03-27
- **Status:** Open
- **Requested by:** UX Design session (Party Mode)
- **Impacts:** PRD (NFR refinement), Architecture, UI
- **Milestone:** M1 (foundation)
- **Description:** All error messages must be cataloged as i18n keys (e.g., `error.isbn.not_found`, `error.label.already_assigned`) with human-written translations in FR and EN. Messages follow the pattern "What happened → Why → What you can do" in plain language. No technical jargon, no HTTP codes, no stack traces exposed to users. Messages maintain the same tone as the rest of the UI. Iteratively improved via milestone retrospectives.
- **Decision:** Pending — to be added to PRD as NFR refinement and CLAUDE.md rule.

### CR-0003: Preventive validation on scan (format, uniqueness, coherence)
- **Date:** 2026-03-27
- **Status:** Open
- **Requested by:** UX Design session (Party Mode)
- **Impacts:** PRD (new FRs), UI, Architecture
- **Milestone:** M1
- **Description:** Add FRs for preventive validation during cataloging: (1) ISBN/ISSN checksum validation client-side before server submission, (2) immediate rejection with detail when scanning an already-assigned V/L label, (3) current title displayed as banner on /catalog so user always knows which title volumes are being attached to, (4) volume↔title and volume↔location associations shown explicitly on both sides. These are trust-building mechanisms identified during UX emotional response design.
- **Decision:** Pending — to be added to PRD as new FRs (FR103+).

### CR-0004: Dedicated cataloging page (/catalog)
- **Date:** 2026-03-27
- **Status:** Open
- **Requested by:** UX Design session (Party Mode)
- **Impacts:** PRD (FR update), Architecture, UI
- **Milestone:** M1
- **Description:** Cataloging and shelving workflows happen on a dedicated /catalog page (Librarian only), separate from the home page. Home page is search + dashboard only. The /catalog page contains the intelligent scan field, dynamic feedback list, current title banner, and session counter. This satisfies FR102 (single-page workflow) naturally. Navigation bar includes a prominent "Catalog" link (Librarian/Admin) and Ctrl+K global shortcut.
- **Decision:** Pending — to be reflected in PRD page structure and FR updates.
