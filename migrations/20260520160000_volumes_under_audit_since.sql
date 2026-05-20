-- CR #237: shelf-audit workflow — per-volume "À contrôler" flag.
--
-- A nullable timestamp. Non-NULL = volume is marked for audit
-- (the timestamp records when it was marked, useful for "stale
-- flag" follow-ups). NULL = not marked / explicitly cleared.
--
-- Cleared MANUALLY only — no auto-clear on volume move, metadata
-- re-fetch, loan return, or any other event (per the user's
-- requirement: "il ne doit pouvoir être désactivé qu'à la main").
--
-- Pairs with the v1.6 #280 organizational-location flag: an audit
-- walk on an organizational container is meaningless because it
-- holds no volumes; the bulk-mark-on-location flow short-circuits
-- on organizational targets.

ALTER TABLE volumes
  ADD COLUMN under_audit_since DATETIME NULL
    COMMENT 'CR #237: non-NULL = volume flagged for shelf audit; cleared manually only';
