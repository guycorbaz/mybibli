---
date: 2026-04-04
trigger: Epic 5 decomposition gap + E2E stabilization commitment from Epic 4 retro
scope: Moderate (backlog reorganization)
status: Approved and applied
workflow: bmad-correct-course (batch mode)
---

# Sprint Change Proposal — Epic 5 Decomposition

## 1. Issue Summary

Epic 5 ("Mes séries et ma collection") was marked as the next epic by the Epic 4 retrospective (2026-04-04), but `epics.md` contained no story decomposition for Epic 5 — only a one-line pitch and the FR/UX-DR lists. Additionally, the Epic 4 retro produced a team agreement:

> **"E2E stabilization = Story 5-1 — no Epic 5 features until E2E pipeline is reliable"**

Without a decomposition, `/bmad-create-story` could not produce story files (no spec source) and the sprint was blocked.

**Issue type:** Sprint planning gap — not a strategic pivot, not a technical drift.

**Evidence:**
- `epics.md:722-726` — Epic 5 without any `#### Story` sub-sections
- `sprint-status.yaml:105-106` — `epic-5: backlog`, zero stories listed
- `epic-4-retro-2026-04-04.md:84-95` — commitments carried to 5-1 (E2E stabilization + CLAUDE.md E2E patterns documentation)
- Memory `project_mybibli_status.md` — "Epic 5 next (starts with story 5-1 E2E stabilization)"

## 2. Impact Analysis

| Artifact | Impact | Action |
|---|---|---|
| `epics.md` | Add 8 `#### Story 5.X` sub-sections under `### Epic 5:` | **Applied** |
| `sprint-status.yaml` | Add 8 story entries under `epic-5` | **Applied** |
| PRD (`prd.md`) | None — all FRs already written and covered | None |
| `architecture.md` | None — no new components, reuses existing patterns (CRUD, autocomplete, pagination, soft delete) | None |
| UX spec | None — UX-DR16/17/18/30 already specified | None |
| Existing code | None — no rollback, Epics 1-4 clean | None |

## 3. Path Forward

**Selected: Option 1 — Direct Adjustment**

Decompose Epic 5 into 8 stories within the existing epic structure. Zero refactoring, zero PRD/architecture changes. Low effort, low risk. Rollback and MVP review options were not applicable (no work to undo, MVP threshold already met at Epic 4).

## 4. Change Scope Applied

### 4.1 Stories added to `epics.md` (Epic 5)

| ID | Title | FRs | UX-DRs | Dependencies |
|---|---|---|---|---|
| 5.1 | E2E Stabilization & Test Pattern Documentation | (tech debt from retro) | — | Blocks 5.2–5.8 |
| 5.2 | Contributor Deletion Guard | FR54 | — | Blocked by 5.1 |
| 5.3 | Series CRUD & Listing | FR36, FR95, FR99 | — | Blocked by 5.1 |
| 5.4 | Title-to-Series Assignment & Gap Detection | FR37, FR38, FR39 | UX-DR16 | Blocked by 5.3 |
| 5.5 | BD Omnibus Multi-Position Volume | FR40 | — | Blocked by 5.4 |
| 5.6 | Browse List/Grid Toggle with Persistent Preference | FR115 | UX-DR17, UX-DR18 | Blocked by 5.1 |
| 5.7 | Similar Titles Section | FR114 | UX-DR30 | Blocked by 5.1 |
| 5.8 | Dewey Code Management | FR118 | — | Blocked by 5.1 |

**Coverage verification:** Epic 5 FRs (FR36-FR40, FR54, FR95, FR99, FR114-FR115, FR118) and UX-DRs (UX-DR16-18, UX-DR30) are all mapped. No orphans.

### 4.2 Sequencing rules

- **5-1 blocks 5-2 through 5-8** (team agreement from Epic 4 retro)
- **5-3 blocks 5-4** (gap detection needs series CRUD)
- **5-4 blocks 5-5** (omnibus extends the gap grid)
- **5-2, 5-6, 5-7, 5-8** are independent of each other once 5-1 is done

### 4.3 Sprint status entries added

Added under `epic-5` in `sprint-status.yaml`:
```yaml
5-1-e2e-stabilization: backlog
5-2-contributor-deletion-guard: backlog
5-3-series-crud-and-listing: backlog
5-4-title-series-assignment-and-gap-detection: backlog
5-5-bd-omnibus-multi-position-volume: backlog
5-6-browse-list-grid-toggle: backlog
5-7-similar-titles-section: backlog
5-8-dewey-code-management: backlog
```

## 5. Implementation Handoff

**Scope classification:** Moderate (backlog reorganization)

**Next actions:**
1. **SM (`/bmad-create-story`)** — generate `5-1-e2e-stabilization.md` as the first story file
2. **Dev (Amelia via `/bmad-dev-story`)** — implement 5-1 once story file is ready
3. **QA (Dana)** — implicit owner of 5-1 (Epic 4 retro action item #2: "Run E2E tests against Docker systematically before milestones")

**Success criteria:**
- [x] `epics.md` contains 8 `#### Story 5.X` sub-sections with BDD acceptance criteria
- [x] `sprint-status.yaml` contains 8 story entries plus `epic-5-retrospective: optional`
- [ ] `/bmad-create-story` can immediately produce `5-1-e2e-stabilization.md`
- [ ] Story 5-1 completes before any Epic 5 feature story enters `in-progress`

## 6. Approval

Approved by Guy on 2026-04-04 via batch-mode review. Edits applied immediately after approval.
