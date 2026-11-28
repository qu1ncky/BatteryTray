// The latest release, as GitHub's API gives it but with only what the apps read of it (src/updater/mod.rs:
// parse_release). Kept in the database for a few minutes, so that the checks of every copy cost GitHub a request
// every 5 minutes at most; and served for an hour more while GitHub doesn't answer. Past that there is no answer, and
// the apps ask GitHub themselves.

export const FRESH = 5 * 60 * 1000;
export const STALE = 60 * 60 * 1000;

/** The release's JSON, from the database or from GitHub; throws when there is none fresh enough to serve. */
export async function latest(env, ctx, now = Date.now()) {
  let kept = null;
  try {
    kept = await env.DB.prepare("SELECT json, fetched FROM release WHERE one = 1").first();
  } catch (e) {
    console.error(JSON.stringify({ message: "can't read the kept release", error: String(e) }));
  }
  if (kept && now - kept.fetched < FRESH) {
    return kept.json;
  }
  let json;
  try {
    json = await fromGitHub(env);
  } catch (e) {
    console.error(JSON.stringify({ message: "GitHub didn't give the release", error: String(e) }));
    if (kept && now - kept.fetched < STALE) {
      return kept.json;
    }
    throw e;
  }
  ctx.waitUntil(keep(env, json, now));
  return json;
}

async function keep(env, json, now) {
  try {
    await env.DB.prepare(
      "INSERT INTO release (one, json, fetched) VALUES (1, ?1, ?2) " +
        "ON CONFLICT (one) DO UPDATE SET json = excluded.json, fetched = excluded.fetched",
    )
      .bind(json, now)
      .run();
  } catch (e) {
    console.error(JSON.stringify({ message: "can't keep the release", error: String(e) }));
  }
}

/** The latest release from GitHub's API, with the token when there is one (60 requests an hour without, per address
 *  - and Workers share theirs). */
export async function fromGitHub(env) {
  const headers = {
    Accept: "application/vnd.github+json",
    "User-Agent": "BatteryTray-updates",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  if (env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${env.GITHUB_TOKEN}`;
  }
  const response = await fetch(`https://api.github.com/repos/${env.REPOSITORY}/releases/latest`, {
    headers,
    signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) {
    throw new Error(`GitHub answered ${response.status}`);
  }
  return trim(await response.json(), env.REPOSITORY);
}

/** What the apps read of a release, as JSON: its tag, page and notes, and the assets that have a size, a SHA-256
 *  digest and a download among this repository's releases. Anything else isn't served. */
export function trim(release, repository) {
  const pages = `https://github.com/${repository}/releases/`;
  const downloads = `${pages}download/`;
  if (typeof release?.tag_name !== "string" || !/^v?\d+\.\d+(\.\d+){0,2}$/.test(release.tag_name)) {
    throw new Error("not a release");
  }
  const assets = (Array.isArray(release.assets) ? release.assets : [])
    .filter(
      (a) =>
        typeof a?.name === "string" &&
        Number.isSafeInteger(a.size) &&
        a.size > 0 &&
        /^sha256:[0-9a-f]{64}$/i.test(a.digest ?? "") &&
        typeof a.browser_download_url === "string" &&
        a.browser_download_url.startsWith(downloads),
    )
    .map(({ name, size, digest, browser_download_url }) => ({ name, size, digest, browser_download_url }));
  if (assets.length === 0) {
    throw new Error("a release without assets to serve");
  }
  return JSON.stringify({
    tag_name: release.tag_name,
    html_url:
      typeof release.html_url === "string" && release.html_url.startsWith(pages) ? release.html_url : `${pages}latest`,
    body: typeof release.body === "string" ? release.body : "",
    assets,
  });
}
