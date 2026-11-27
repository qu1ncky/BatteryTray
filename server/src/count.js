// Counting the copies of the app in use. An update check carries the app's version, system and processor in its
// User-Agent - "BatteryTray/2.5.0 (Windows 10.0.26100; x64)", "BatteryTray/2.5.0 (macOS 26.6; arm64)" - and in
// BatteryTray-Install the random number the copy made up on its first check. The country is Cloudflare's guess from
// the address; the address itself is never kept. A check without these is answered all the same, and not counted.

const AGENT = /^BatteryTray\/(\d{1,4}\.\d{1,4}\.\d{1,6}) \((Windows|macOS) (\d{1,6}(?:\.\d{1,6}){0,3}); ([a-z0-9_]{1,12})\)$/;
const ID = /^[0-9a-f]{32}$/;
const DAY = 86_400_000;

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

/** Counts the copy that asked: it was seen today, with what it said about itself. Writes nothing when it was already
 *  seen today and nothing changed. */
export async function record(request, env, now = Date.now()) {
  const copy = about(request);
  if (!copy) {
    return;
  }
  const today = day(now);
  await env.DB.batch([
    env.DB.prepare(
      "INSERT INTO installs (id, first_seen, last_seen, version, os, os_version, arch, country) " +
        "VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7) " +
        "ON CONFLICT (id) DO UPDATE SET last_seen = excluded.last_seen, version = excluded.version, os = excluded.os, " +
        "os_version = excluded.os_version, arch = excluded.arch, country = excluded.country " +
        "WHERE installs.last_seen <> excluded.last_seen OR installs.version <> excluded.version " +
        "OR installs.os <> excluded.os OR installs.os_version <> excluded.os_version OR installs.arch <> excluded.arch " +
        "OR installs.country <> excluded.country",
    ).bind(copy.id, today, copy.version, copy.os, copy.osVersion, copy.arch, copy.country),
    env.DB.prepare("INSERT OR IGNORE INTO seen (day, id) VALUES (?1, ?2)").bind(today, copy.id),
  ]);
}

/** Each finished day's numbers into `daily`, for the days of the last 60 that don't have them yet, from the day the
 *  first copy was seen; then the days seen over 90 days ago are let go. Run by the cron trigger just after midnight
 *  UTC. A run after a pause catches up 10 days at a time: 4 statements a day, and a Worker on the free plan may make
 *  50 queries a run. D1 takes no compound SELECT of that many terms, hence one statement per breakdown. */
export async function sumUp(env, now = Date.now()) {
  const today = day(now);
  const first = (await env.DB.prepare("SELECT min(first_seen) AS first FROM installs").first())?.first;
  const statements = [];
  if (first) {
    const from = first > before(today, 60) ? first : before(today, 60);
    const done = new Set(
      (await env.DB.prepare("SELECT day FROM daily WHERE metric = 'users' AND day >= ?1").bind(from).all()).results.map(
        (r) => r.day,
      ),
    );
    let days = 0;
    for (let d = from; d < today && days < 10; d = before(d, -1)) {
      if (done.has(d)) {
        continue;
      }
      days++;
      statements.push(
        env.DB.prepare(
          "INSERT OR REPLACE INTO daily (day, metric, key, value) VALUES " +
            "(?1, 'users', '', (SELECT count(*) FROM seen WHERE day = ?1)), " +
            "(?1, 'users7', '', (SELECT count(DISTINCT id) FROM seen WHERE day BETWEEN ?2 AND ?1)), " +
            "(?1, 'users30', '', (SELECT count(DISTINCT id) FROM seen WHERE day BETWEEN ?3 AND ?1)), " +
            "(?1, 'new', '', (SELECT count(*) FROM installs WHERE first_seen = ?1))",
        ).bind(d, before(d, 6), before(d, 29)),
      );
      for (const column of ["os", "country", "version"]) {
        statements.push(
          env.DB.prepare(
            `INSERT OR REPLACE INTO daily (day, metric, key, value) SELECT ?1, '${column}', i.${column}, count(*) ` +
              `FROM seen s JOIN installs i ON i.id = s.id WHERE s.day = ?1 GROUP BY i.${column}`,
          ).bind(d),
        );
      }
    }
  }
  statements.push(env.DB.prepare("DELETE FROM seen WHERE day < ?1").bind(before(today, 90)));
  await env.DB.batch(statements);
}
