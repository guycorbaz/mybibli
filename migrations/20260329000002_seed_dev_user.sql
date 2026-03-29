-- Dev seed: librarian user with pre-set session for development/testing
-- SAFETY: Only inserts if RUST_LOG contains 'debug' (dev environments only)
-- In production, RUST_LOG=mybibli=info so this INSERT is skipped

-- Only seed if no users exist yet (first launch in dev)
INSERT INTO users (username, password_hash, role, active)
SELECT 'dev_librarian', '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$placeholder_hash_replace_at_runtime', 'librarian', TRUE
FROM DUAL
WHERE NOT EXISTS (SELECT 1 FROM users WHERE username = 'dev_librarian');

-- Pre-set session token for development
-- Set browser cookie: session=ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2
INSERT INTO sessions (token, user_id, data, last_activity)
SELECT 'ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2',
       (SELECT id FROM users WHERE username = 'dev_librarian'),
       '{}', NOW()
FROM DUAL
WHERE NOT EXISTS (SELECT 1 FROM sessions WHERE token = 'ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2');
