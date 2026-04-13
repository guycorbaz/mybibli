-- Widen titles.dewey_code from VARCHAR(15) to VARCHAR(32).
-- Rationale: BnF UNIMARC 676$a can hold extended DDC notations up to ~22 chars
-- (cross-classifications, period-separated subdivisions). The initial 15-char
-- ceiling risked silent truncation on non-strict MariaDB or UPDATE failures on
-- strict mode when the async metadata fetch resolved long codes.
-- Story 5-8 code review decision (2026-04-12): option 3 (widen column).
ALTER TABLE titles MODIFY COLUMN dewey_code VARCHAR(32) NULL;
