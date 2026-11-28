-- Each copy of the app: the random number it made up on its first update check, the days (UTC) it was first and last
-- seen, and what it said about itself the last time. The country comes from Cloudflare; the address isn't kept.
CREATE TABLE installs (
  id TEXT PRIMARY KEY,
  first_seen TEXT NOT NULL,
  last_seen TEXT NOT NULL,
  version TEXT NOT NULL,
  os TEXT NOT NULL,
  os_version TEXT NOT NULL,
  arch TEXT NOT NULL,
  country TEXT NOT NULL
);

-- The days each copy checked for updates, for the users of a day and of the 7 and 30 days up to it. Kept for 90 days;
-- what they add up to stays in `daily`.
CREATE TABLE seen (
  day TEXT NOT NULL,
  id TEXT NOT NULL,
  PRIMARY KEY (day, id)
) WITHOUT ROWID;

-- Each finished day's numbers: its users, those of the 7 and 30 days up to it, the copies first seen that day, and
-- the day's users by system, country and version of the app
CREATE TABLE daily (
  day TEXT NOT NULL,
  metric TEXT NOT NULL,
  key TEXT NOT NULL DEFAULT '',
  value INTEGER NOT NULL,
  PRIMARY KEY (day, metric, key)
) WITHOUT ROWID;

-- The latest release as GitHub last gave it, so that a check doesn't wait for GitHub and GitHub's limits are kept to
CREATE TABLE release (
  one INTEGER PRIMARY KEY CHECK (one = 1),
  json TEXT NOT NULL,
  fetched INTEGER NOT NULL
);
