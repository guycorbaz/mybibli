# Story 16.3: Purchase price proposals

Status: done

GitHub issue: #639 (type:change-request). Covers: FR168. Epic 16; ships in v1.26.0.

## Story

As a librarian whose purchase prices are mostly empty,
I want the agent to propose them, even without a value estimate, and to flag those that look wrong,
so that the amounts fill up while I keep the decision.

## Decisions (project lead, 2026-11-06 — #639)

Two amounts per volume only, labels unchanged. The list price may fill an empty purchase price. The agent can propose a purchase price alone, and can propose replacing an inconsistent one: zero or negligible, a scale error, or a clear gap with an attested list price. A librarian decides.

## Design decisions (dev, 2026-11-06)

- **D1 — no new tool or kind.** `propose_volume_value` already carries `list_price`; `current_value` becomes optional, at least one amount being required. Stored lines of earlier filings read as before.
- **D2 — bounds (Rule #20, positive shapes).** `purchase_price_problem`: `negligible` (at or below the negligible amount, against a higher list price), `gap` (from the gap factor), `scale` (a gap of 10 or more, a label only), only in the default currency. Anything else is a real price and the filing is refused, even a low one: by default, 8.01 against a list price of 24 is left alone.
- **D8 — the thresholds are settings (project lead, 2026-11-06).** Admin → System: `purchase_negligible_amount` (default 1.00, zero or more, in the default currency) and `purchase_gap_factor` (default 3, whole, 2 to 100: a factor of 1 would make every price replaceable). Read at each filing, no cache; both saved in one transaction; an invalid stored value reads as its default. The review copy and the tool description name no figure, and a refusal gives the live ones. A change does not touch proposals already filed: acceptance only checks that the price is the one filed.
- **D3 — optional keys on the list-price line.** `replaces` + `inconsistent` on a replacement; `note` + `fingerprint` on a line filed alone (`list:<cents><CUR>`). The value line keeps carrying them when present, so existing equivalence keys do not change.
- **D4 — staleness.** The list-price line stales when the live purchase price differs from `replaces` (absent = no price), which covers both "set meanwhile" and "changed meanwhile". The stale copy now says "changed".
- **D5 — a list price filed alone cannot be removed** (`Keep::LastLine`); reject instead. A date is written only with a value.
- **D7 — equivalence stays "the same amounts" (15-5 D5).** A rejected list price alone does not block the same list price filed with a value, nor the reverse: the fingerprint differs. Accepted; the librarian rejects again if needed.
- **D6 — the audit row** records what changed, and now also the old purchase price and its currency (#630, item 6).

## Acceptance Criteria

1. **AC1.** `current_value` optional; neither amount is refused and nothing is filed.
2. **AC2.** `list_price` accepted on an empty purchase price and on the listed shapes; any other case refused and explained.
3. **AC3.** The review shows the volume, the old and new amounts, why the existing one looks wrong, the note and the sources.
4. **AC4.** Acceptance stales if the purchase price changed; a price alone writes neither the value nor its date.
5. **AC5.** Rule #22 holds (the volume is locked by primary key, unchanged). Unit, DB integration (`tests/proposal_volume_value.rs`) and E2E (`volume-value-proposal.spec.ts`, Rule #23).
