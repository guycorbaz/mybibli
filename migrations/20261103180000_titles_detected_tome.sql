-- Story 13-10 (#579): the tome number.
--
-- A metadata title carrying a tome marker ("Tome 3 : …", "Titeuf T12 : …")
-- is stored without it; the number goes to the series position when the
-- title already sits in a series, or waits here when it sits in none.
--
-- NON-AUTHORITATIVE hint: displayed as "Tome N (detected)" on the title
-- page, pre-fills the position when a series is attached there, then is
-- cleared. `title_series.position_number` stays the only authoritative
-- tome number. Writing or clearing it bumps no `version`.

ALTER TABLE titles ADD COLUMN detected_tome INT NULL DEFAULT NULL;
