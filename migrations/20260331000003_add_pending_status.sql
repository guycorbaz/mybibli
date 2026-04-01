-- Add status column to pending_metadata_updates for tracking resolution outcome
ALTER TABLE pending_metadata_updates
    ADD COLUMN status VARCHAR(20) NOT NULL DEFAULT 'pending';
