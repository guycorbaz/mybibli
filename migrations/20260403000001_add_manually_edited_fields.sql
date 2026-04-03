-- Add manually_edited_fields column to track which metadata fields
-- were manually edited by the user (for per-field confirmation on re-download).
-- Stored as JSON array of field names, e.g. ["publisher","description"].
ALTER TABLE titles ADD COLUMN manually_edited_fields JSON NULL;
