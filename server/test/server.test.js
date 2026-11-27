// The update server against an in-memory D1 and a stand-in for GitHub's API: what an update check gets, what is
// counted, the day's numbers and the owners' page.

import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, test } from "node:test";
import { about, before, record, sumUp } from "../src/count.js";
import worker from "../src/index.js";
import { FRESH, STALE, latest, trim } from "../src/release.js";
import { data } from "../src/stats.js";
import { database } from "./d1.js";

const REPOSITORY = "qu1ncky/BatteryTray";
const DOWNLOADS = `https://github.com/${REPOSITORY}/releases/download`;
const DIGEST = `sha256:${"ab".repeat(32)}`;
const ID = "0123456789abcdef0123456789abcdef";
const WINDOWS = "BatteryTray/2.5.0 (Windows 10.0.26100; x64)";
const MAC = "BatteryTray/2.5.0 (macOS 26.6; arm64)";

/** A release as GitHub's API gives it, with more than the apps read. */
function githubRelease(tag = "v2.5.0") {
  return {
    tag_name: tag,
    name: `BatteryTray ${tag}`,
    html_url: `https://github.com/${REPOSITORY}/releases/tag/${tag}`,
    body: "What's new",
    author: { login: "someone" },
    assets: [
      { name: "BatteryTray.exe", size: 12333056, digest: DIGEST, browser_download_url: `${DOWNLOADS}/${tag}/BatteryTray.exe`, uploader: {} },
      { name: "BatteryTray-macOS.zip", size: 4084154, digest: DIGEST, browser_download_url: `${DOWNLOADS}/${tag}/BatteryTray-macOS.zip` },
      { name: "elsewhere.exe", size: 10, digest: DIGEST, browser_download_url: "https://example.com/elsewhere.exe" },
      { name: "undigested.zip", size: 10, browser_download_url: `${DOWNLOADS}/${tag}/undigested.zip` },
    ],
  };
}

let env, github, calls, waits;

beforeEach(() => {
  env = { DB: database(), REPOSITORY, SERVE: "on", STATS_KEY: "the-key" };
  github = { status: 200, release: githubRelease() };
  calls = [];
  waits = [];
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), headers: init?.headers ?? {} });
    if (github.status === 0) {
      throw new TypeError("network down");
    }
    return Response.json(github.release, { status: github.status });
  };
});

afterEach(() => {
  delete globalThis.fetch;
});

const ctx = { waitUntil: (promise) => waits.push(promise) };

async function settled() {
  while (waits.length) {
    await waits.shift();
  }
}

function request(path, { agent = WINDOWS, id = ID, country = "RU", method = "GET", headers = {} } = {}) {
  const r = new Request(`https://updates.example.workers.dev${path}`, {
    method,
    headers: { ...(agent ? { "User-Agent": agent } : {}), ...(id ? { "BatteryTray-Install": id } : {}), ...headers },
  });
  Object.defineProperty(r, "cf", { value: country ? { country } : undefined });
  return r;
}

async function check(options) {
  const response = await worker.fetch(request("/v1/latest", options), env, ctx);
  await settled();
  return response;
}

const rows = (sql, ...params) => env.DB.sqlite.prepare(sql).all(...params).map((row) => ({ ...row }));

describe("a release", () => {
  test("keeps what the apps read, and only downloads from the repository's releases with a digest", () => {
    const served = JSON.parse(trim(githubRelease(), REPOSITORY));
    assert.deepEqual(Object.keys(served), ["tag_name", "html_url", "body", "assets"]);
    assert.equal(served.tag_name, "v2.5.0");
    assert.deepEqual(
      served.assets.map((a) => a.name),
      ["BatteryTray.exe", "BatteryTray-macOS.zip"],
    );
    assert.deepEqual(Object.keys(served.assets[0]), ["name", "size", "digest", "browser_download_url"]);
  });

  test("isn't served without a tag that is a version or without an asset to serve", () => {
    assert.throws(() => trim({ ...githubRelease(), tag_name: "nightly" }, REPOSITORY));
    assert.throws(() => trim({ ...githubRelease(), assets: githubRelease().assets.slice(2) }, REPOSITORY));
    assert.throws(() => trim(null, REPOSITORY));
  });

  test("points its page at the repository's releases, whatever GitHub's says", () => {
    const served = JSON.parse(trim({ ...githubRelease(), html_url: "https://example.com/" }, REPOSITORY));
    assert.equal(served.html_url, `https://github.com/${REPOSITORY}/releases/latest`);
  });
});

describe("an update check", () => {
  test("gets the latest release, as GitHub gave it to the server with its token", async () => {
    env.GITHUB_TOKEN = "token";
    const response = await check();
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("Cache-Control"), "no-store");
    const served = await response.json();
    assert.equal(served.tag_name, "v2.5.0");
    assert.equal(served.assets.length, 2);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].url, `https://api.github.com/repos/${REPOSITORY}/releases/latest`);
    assert.equal(calls[0].headers.Authorization, "Bearer token");
  });

  test("asks GitHub again only after a few minutes", async () => {
    const now = Date.now();
    await latest(env, ctx, now);
    await settled();
    github.release = githubRelease("v2.5.1");
    assert.equal(JSON.parse(await latest(env, ctx, now + FRESH - 1000)).tag_name, "v2.5.0");
    assert.equal(calls.length, 1);
    assert.equal(JSON.parse(await latest(env, ctx, now + FRESH + 1000)).tag_name, "v2.5.1");
    assert.equal(calls.length, 2);
  });

  test("gets the kept release for an hour while GitHub fails, then nothing, so the app asks GitHub", async () => {
    const now = Date.now();
    await latest(env, ctx, now);
    await settled();
    for (const status of [0, 403, 500]) {
      github.status = status;
      assert.equal(JSON.parse(await latest(env, ctx, now + STALE - 1000)).tag_name, "v2.5.0");
      await assert.rejects(latest(env, ctx, now + STALE + 1000));
    }
  });

  test("is answered 503 when GitHub gives nothing usable and nothing is kept", async () => {
    github.release = { message: "Not Found" };
    assert.equal((await check()).status, 503);
    github.status = 0;
    assert.equal((await check()).status, 503);
  });

  test("is answered from GitHub when the database fails, and 503 when the switch is off", async () => {
    env.DB.broken = true;
    assert.equal((await check()).status, 200);
    env.DB.broken = false;
    env.SERVE = "off";
    assert.equal((await check()).status, 503);
    assert.equal(rows("SELECT * FROM installs").length, 1, "still counted");
  });

  test("counts the copy once a day, with its version, system, processor and country", async () => {
    await check();
    await check();
    await check({ id: "ffffffffffffffffffffffffffffffff", agent: MAC, country: "DE" });
    const installs = rows("SELECT id, version, os, os_version, arch, country FROM installs ORDER BY id");
    assert.deepEqual(installs, [
      { id: ID, version: "2.5.0", os: "Windows", os_version: "10.0.26100", arch: "x64", country: "RU" },
      { id: "f".repeat(32), version: "2.5.0", os: "macOS", os_version: "26.6", arch: "arm64", country: "DE" },
    ]);
    assert.equal(rows("SELECT * FROM seen").length, 2);
  });

  test("isn't counted without a random number or a User-Agent of the app, and is answered all the same", async () => {
    for (const options of [{ id: null }, { id: "not-a-number" }, { id: ID.toUpperCase() }, { agent: null },
      { agent: "BatteryTray/2.4.2" }, { agent: "curl/8.7.1" }, { agent: "BatteryTray/2.5.0 (Linux 6.1; x64)" }]) {
      assert.equal((await check(options)).status, 200);
    }
    assert.equal(rows("SELECT * FROM installs").length, 0);
  });

  test("keeps what changed: a new version, another country", async () => {
    const now = Date.parse("2026-12-01T10:00:00Z");
    await record(request("/v1/latest"), env, now);
    await record(request("/v1/latest", { agent: "BatteryTray/2.6.0 (Windows 10.0.26200; x64)", country: "KZ" }), env, now + 3600_000);
    await record(request("/v1/latest"), env, now + 86_400_000);
    const [copy] = rows("SELECT * FROM installs");
    assert.equal(copy.first_seen, "2026-12-01");
    assert.equal(copy.last_seen, "2026-12-02");
    assert.equal(copy.version, "2.5.0");
    assert.equal(copy.country, "RU");
    assert.deepEqual(rows("SELECT day FROM seen ORDER BY day").map((r) => r.day), ["2026-12-01", "2026-12-02"]);
  });

  test("of an unknown country is counted as XX", () => {
    assert.equal(about(request("/v1/latest", { country: null })).country, "XX");
    assert.equal(about(request("/v1/latest", { country: "T1" })).country, "T1");
  });
});

describe("the day's numbers", () => {
  /** Copies seen on days given as offsets from 2026-12-31. */
  async function seen(copies) {
    const base = Date.parse("2026-12-31T12:00:00Z");
    for (const { id, days, agent = WINDOWS, country = "RU" } of copies) {
      for (const d of days) {
        await record(request("/v1/latest", { id, agent, country }), env, base + d * 86_400_000);
      }
    }
  }
  const metric = (day, name, key = "") =>
    rows("SELECT value FROM daily WHERE day = ? AND metric = ? AND key = ?", day, name, key)[0]?.value;

  test("are the users of the day, of 7 and 30 days, the new copies, by system, country and version", async () => {
    const a = "a".repeat(32), b = "b".repeat(32), c = "c".repeat(32);
    await seen([
      { id: a, days: [-20, -3, -1] },
      { id: b, days: [-1], agent: MAC, country: "DE" },
      { id: c, days: [-10] },
    ]);
    for (let run = 0; run < 3; run++) {
      await sumUp(env, Date.parse(`2027-01-01T0${run}:10:00Z`));
    }
    assert.equal(metric("2026-12-30", "users"), 2);
    assert.equal(metric("2026-12-30", "users7"), 2);
    assert.equal(metric("2026-12-30", "users30"), 3);
    assert.equal(metric("2026-12-30", "new"), 1);
    assert.equal(metric("2026-12-30", "os", "Windows"), 1);
    assert.equal(metric("2026-12-30", "os", "macOS"), 1);
    assert.equal(metric("2026-12-30", "country", "DE"), 1);
    assert.equal(metric("2026-12-30", "version", "2.5.0"), 2);
    assert.equal(metric("2026-12-21", "users"), 1, "the days between are caught up");
    assert.equal(metric("2026-12-28", "users"), 1);
    assert.equal(metric("2026-12-25", "users"), 0);
    assert.equal(metric("2026-12-10", "users"), undefined, "nothing before the first copy was seen");
    assert.equal(metric("2026-12-31", "users"), 0);
    assert.equal(metric("2027-01-01", "users"), undefined, "the day isn't over");
  });

  test("catch up 10 days at a time, each day once, and the days seen over 90 days ago are let go", async () => {
    const summed = () => rows("SELECT count(*) AS n FROM daily WHERE metric = 'users'")[0].n;
    await seen([{ id: "a".repeat(32), days: [-120, -1] }]);
    await sumUp(env, Date.parse("2027-01-01T00:10:00Z"));
    assert.equal(summed(), 10, "from 60 days back");
    assert.equal(metric("2026-11-02", "users"), 0);
    assert.deepEqual(rows("SELECT day FROM seen").map((r) => r.day), ["2026-12-30"]);
    for (let run = 1; run < 6; run++) {
      await sumUp(env, Date.parse(`2027-01-01T0${run}:00:00Z`));
    }
    assert.equal(summed(), 60);
    assert.equal(metric("2026-12-30", "users"), 1);
    await sumUp(env, Date.parse("2027-01-01T08:00:00Z"));
    assert.equal(summed(), 60, "nothing twice");
    await sumUp(env, Date.parse("2027-01-02T00:10:00Z"));
    assert.equal(summed(), 61);
  });

  test("are run by the cron trigger", async () => {
    await seen([{ id: "a".repeat(32), days: [-1] }]);
    await worker.scheduled({ scheduledTime: Date.parse("2027-01-01T00:10:00Z") }, env, ctx);
    await settled();
    assert.equal(metric("2026-12-30", "users"), 1);
  });
});

describe("the owners' page", () => {
  test("is a page without numbers, not to be indexed or framed", async () => {
    const response = await worker.fetch(request("/stats"), env, ctx);
    assert.equal(response.status, 200);
    assert.match(response.headers.get("Content-Type"), /^text\/html/);
    assert.match(response.headers.get("Content-Security-Policy"), /frame-ancestors 'none'/);
    assert.equal(response.headers.get("X-Robots-Tag"), "noindex");
    assert.match(await response.text(), /\/stats\/data/);
  });

  test("gets its numbers only with the key", async () => {
    for (const headers of [{}, { Authorization: "Bearer wrong" }, { Authorization: "the-key" }]) {
      assert.equal((await worker.fetch(request("/stats/data", { headers }), env, ctx)).status, 403);
    }
    delete env.STATS_KEY;
    assert.equal((await worker.fetch(request("/stats/data", { headers: { Authorization: "Bearer " } }), env, ctx)).status, 403);
  });

  test("gets the users of today, 7 and 30 days, by country, system and version, and the latest release", async () => {
    const now = Date.parse("2026-12-31T12:00:00Z");
    await record(request("/v1/latest", { id: "a".repeat(32) }), env, now);
    await record(request("/v1/latest", { id: "b".repeat(32), agent: MAC, country: "DE" }), env, now - 5 * 86_400_000);
    await record(request("/v1/latest", { id: "c".repeat(32) }), env, now - 20 * 86_400_000);
    await latest(env, ctx, now);
    await settled();
    for (let run = 0; run < 3; run++) {
      await sumUp(env, now + run * 60_000);
    }
    const response = await data(request("/stats/data", { headers: { Authorization: "Bearer the-key" } }), env, now);
    assert.equal(response.status, 200);
    const numbers = await response.json();
    assert.equal(numbers.today, "2026-12-31");
    assert.equal(numbers.latest, "v2.5.0");
    assert.deepEqual(numbers.users, { day: 1, week: 2, month: 3, installs: 3, new_day: 1, new_week: 2 });
    assert.deepEqual(numbers.countries, [{ country: "RU", count: 2 }, { country: "DE", count: 1 }]);
    assert.deepEqual(numbers.versions, [{ version: "2.5.0", count: 2 }]);
    assert.equal(numbers.systems.length, 2);
    assert.ok(numbers.history.some((d) => d.day === "2026-12-26" && d.users === 1 && d.new === 1));
  });
});

describe("anything else", () => {
  test("is not found, and only GET and HEAD are answered", async () => {
    assert.equal((await worker.fetch(request("/"), env, ctx)).status, 404);
    assert.equal((await worker.fetch(request("/v1/latest", { method: "POST" }), env, ctx)).status, 405);
  });

  test("days count back in UTC", () => {
    assert.equal(before("2027-01-01", 1), "2026-12-31");
    assert.equal(before("2026-12-31", -1), "2027-01-01");
    assert.equal(before("2026-03-30", 29), "2026-03-01");
  });
});
