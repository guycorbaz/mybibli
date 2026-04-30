-- Story 8-8 review pass-1 follow-up: Step 2 visited sentinel.
-- The wizard's Step 2 (provider keys) is entirely optional — the user
-- can skip every provider — so empty-key state is indistinguishable
-- from "user has not visited Step 2 yet". Without a dedicated
-- sentinel, the resolver loops the user back to Step 2 forever.
-- Mirrors `setup_step_3_done` from migration 20260429000000.
INSERT IGNORE INTO settings (setting_key, setting_value) VALUES
    ('setup_step_2_done', '0');
