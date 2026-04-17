-- Story 7-3: per-user stored UI-language preference.
-- NULL = "no stored preference" (follow cookie / Accept-Language / default `fr`).
-- Only `fr` and `en` are accepted — adding a locale requires a future ALTER.
ALTER TABLE users
    ADD COLUMN preferred_language ENUM('fr', 'en') NULL DEFAULT NULL AFTER role;
