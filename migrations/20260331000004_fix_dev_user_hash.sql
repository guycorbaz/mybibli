-- Replace dev_librarian with admin user (username: admin, password: admin, role: admin)
-- Update username, password hash, and role
UPDATE users
SET username = 'admin',
    password_hash = '$argon2id$v=19$m=19456,t=2,p=1$4g83LVDxAaFJOYMH7jrQCA$rzWkSQWhV9koCi5hJu2BVQa9LhcZHCpvJnxNBrU1nBw',
    role = 'admin'
WHERE username = 'dev_librarian' AND deleted_at IS NULL;
