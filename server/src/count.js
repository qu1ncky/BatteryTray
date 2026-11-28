// Counting the copies of the app in use. An update check carries the app's version, system and processor in its
// User-Agent - "BatteryTray/2.5.0 (Windows 10.0.26100; x64)", "BatteryTray/2.5.0 (macOS 26.6; arm64)" - and in
// BatteryTray-Install the random number the copy made up on its first check. The country is Cloudflare's guess from
// the address; the address itself is never kept. A check without these is answered all the same, and not counted.
//
// A copy counts as a user only once its checks span KEPT: the app checks half a minute after it starts and then every
// 6 hours, so a copy in use is seen again within the day or the next time it starts. A CI run (its job ends within
// 45 minutes), a quick try, a test by hand or a script that invents numbers and doesn't come back never counts, and
// is let go after a week.

const AGENT = /^BatteryTray\/(\d{1,4}\.\d{1,4}\.\d{1,6}) \((Windows|macOS) (\d{1,6}(?:\.\d{1,6}){0,3}); ([a-z0-9_]{1,12})\)$/;
const ID = /^[0-9a-f]{32}$/;
const DAY = 86_400_000;
export const KEPT = 3 * 3600 * 1000;
const LET_GO = 7 * DAY;

/** The SQL condition that the copy of table alias `i` counts as a user. */
export function kept(i = "i") {
  return `(${i}.last_at - ${i}.first_at >= ${KEPT})`;
}

/** The copy that asked, or null for a request that doesn't say. */
export function about(request) {
  const id = request.headers.get("BatteryTray-Install") ?? "";
  const agent = AGENT.exec(request.headers.get("User-Agent") ?? "");
  if (!ID.test(id) || !agent) {
    return null;
  }
  const [, version, os, osVersion, arch] = agent;
  const country = /^[A-Z0-9]{2}$/.test(request.cf?.country ?? "") ? request.cf.country : "XX";
  return { id, version, os, osVersion, arch, country };
}

/** The day of a moment in UTC, "2026-11-27". */
export function day(moment) {
  return new Date(moment).toISOString().slice(0, 10);
}

/** The day `days` before another. */
export function before(date, days) {
  return day(Date.parse(`${date}T00:00:00Z`) - days * DAY);
}

/** Counts the copy that asked: it was seen now, with what it said about itself. Until it counts as a user every check
 *  is written down; after that only the first of a day, or one that says something new. */
export async function record(request, env, now = Date.now()) {
  const copy = about(request);
  if (!copy) {
    return;
  }
  const today = day(now);
  await env.DB.batch([
    env.DB.prepare(
      "INSERT INTO installs (id, first_seen, last_seen, first_at, last_at, version, os, os_version, arch, country) " +
        "VALUES (?1, ?2, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8) " +
        "ON CONFLICT (id) DO UPDATE SET last_seen = excluded.last_seen, last_at = excluded.last_at, " +
        "version = excluded.version, os = excluded.os, os_version = excluded.os_version, arch = excluded.arch, " +
        "country = excluded.country " +
        `WHERE NOT ${kept("installs")} OR installs.last_seen <> excluded.last_seen ` +
        "OR installs.version <> excluded.version OR installs.os <> excluded.os " +
        "OR installs.os_version <> excluded.os_version OR installs.arch <> excluded.arch " +
        "OR installs.country <> excluded.country",
    ).bind(copy.id, today, now, copy.version, copy.os, copy.osVersion, copy.arch, copy.country),
    env.DB.prepare("INSERT OR IGNORE INTO seen (day, id) VALUES (?1, ?2)").bind(today, copy.id),
  ]);
}

/** Each finished day's numbers into `daily`, counting only the copies that are users: the days of the last 60 that
 *  don't have them yet, from the day the first user was seen, and always the last 3 again, as a copy seen on them may
 *  have become a user since. Then the copies that never became one within a week are let go, and the days seen over
 *  90 days ago. Run by the cron trigger just after midnight UTC. A run does 8 days at most, 5 statements each: a
 *  Worker on the free plan may make 50 queries a run, and D1 takes no compound SELECT of that many terms. */
export async function sumUp(env, now = Date.now()) {
  const today = day(now);
  const first = (await env.DB.prepare(`SELECT min(i.first_seen) AS first FROM installs i WHERE ${kept()}`).first())
    ?.first;
  const statements = [];
  if (first) {
    const from = first > before(today, 60) ? first : before(today, 60);
    const done = new Set(
      (await env.DB.prepare("SELECT day FROM daily WHERE metric = 'users' AND day >= ?1").bind(from).all()).results.map(
        (r) => r.day,
      ),
    );
    const again = [3, 2, 1].map((n) => before(today, n)).filter((d) => d >= from);
    const missing = [];
    for (let d = from; d < (again[0] ?? today) && missing.length < 8 - again.length; d = before(d, -1)) {
      if (!done.has(d)) {
        missing.push(d);
      }
    }
    for (const d of [...missing, ...again]) {
      statements.push(
        env.DB.prepare(
          "INSERT OR REPLACE INTO daily (day, metric, key, value) VALUES " +
            "(?1, 'users', '', (SELECT count(*) FROM seen s JOIN installs i ON i.id = s.id " +
            `WHERE s.day = ?1 AND ${kept()})), ` +
            "(?1, 'users7', '', (SELECT count(DISTINCT s.id) FROM seen s JOIN installs i ON i.id = s.id " +
            `WHERE s.day BETWEEN ?2 AND ?1 AND ${kept()})), ` +
            "(?1, 'users30', '', (SELECT count(DISTINCT s.id) FROM seen s JOIN installs i ON i.id = s.id " +
            `WHERE s.day BETWEEN ?3 AND ?1 AND ${kept()})), ` +
            `(?1, 'new', '', (SELECT count(*) FROM installs i WHERE i.first_seen = ?1 AND ${kept()}))`,
        ).bind(d, before(d, 6), before(d, 29)),
        // The breakdowns afresh: a copy's version or country may have changed since the day was last summed up
        env.DB.prepare("DELETE FROM daily WHERE day = ?1 AND metric IN ('os', 'country', 'version')").bind(d),
      );
      for (const column of ["os", "country", "version"]) {
        statements.push(
          env.DB.prepare(
            `INSERT INTO daily (day, metric, key, value) SELECT ?1, '${column}', i.${column}, count(*) ` +
              `FROM seen s JOIN installs i ON i.id = s.id WHERE s.day = ?1 AND ${kept()} GROUP BY i.${column}`,
          ).bind(d),
        );
      }
    }
  }
  const gone = `SELECT i.id FROM installs i WHERE NOT ${kept()} AND i.last_at < ?1`;
  statements.push(
    env.DB.prepare(`DELETE FROM seen WHERE id IN (${gone})`).bind(now - LET_GO),
    env.DB.prepare(`DELETE FROM installs WHERE id IN (${gone})`).bind(now - LET_GO),
    env.DB.prepare("DELETE FROM seen WHERE day < ?1").bind(before(today, 90)),
  );
  await env.DB.batch(statements);
}
