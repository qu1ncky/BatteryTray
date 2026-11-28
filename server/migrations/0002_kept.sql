-- When each copy was first and last seen, to the millisecond (server time). A copy counts as someone's only once its
-- checks span 3 hours (count.js, KEPT): a CI run, a quick try or a script that doesn't come back never gets there.
-- The rows from before have 0 for both, so they don't count, and they go with the copies that never did.
ALTER TABLE installs ADD COLUMN first_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE installs ADD COLUMN last_at INTEGER NOT NULL DEFAULT 0;
