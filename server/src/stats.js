// The owners' page: how many copies are in use, over time, by country, system and version. The page itself holds no
// numbers; it asks /stats/data for them with the key (the STATS_KEY secret), which it takes once from the address's
// #key=… part - never sent to the server, so never in a log - and then keeps in the browser.

import { before, day, kept } from "./count.js";

/** The numbers, for the page; 403 without the key. Only the copies that count as users (count.js, kept) are in them;
 *  `pending` says how many others were seen this week: CI runs, quick tries, copies not yet seen twice. */
export async function data(request, env, now = Date.now()) {
  if (!(await allowed(request, env))) {
    return new Response("Forbidden\n", { status: 403, headers: { "Cache-Control": "no-store" } });
  }
  const today = day(now);
  const week = before(today, 6);
  const month = before(today, 29);
  const [users, history, systems, countries, versions, release] = await env.DB.batch([
    env.DB.prepare(
      `SELECT coalesce(sum(${kept()} AND i.last_seen = ?1), 0) AS day, ` +
        `coalesce(sum(${kept()} AND i.last_seen >= ?2), 0) AS week, ` +
        `coalesce(sum(${kept()} AND i.last_seen >= ?3), 0) AS month, coalesce(sum(${kept()}), 0) AS installs, ` +
        `coalesce(sum(${kept()} AND i.first_seen = ?1), 0) AS new_day, ` +
        `coalesce(sum(${kept()} AND i.first_seen >= ?2), 0) AS new_week, ` +
        `coalesce(sum(NOT ${kept()} AND i.last_seen >= ?2), 0) AS pending FROM installs i`,
    ).bind(today, week, month),
    env.DB.prepare("SELECT day, metric, value FROM daily WHERE key = '' AND day >= ?1 ORDER BY day").bind(
      before(today, 365),
    ),
    env.DB.prepare(
      "SELECT i.os, i.os_version, i.arch, count(*) AS count FROM installs i " +
        `WHERE i.last_seen >= ?1 AND ${kept()} GROUP BY i.os, i.os_version, i.arch`,
    ).bind(month),
    env.DB.prepare(
      `SELECT i.country, count(*) AS count FROM installs i WHERE i.last_seen >= ?1 AND ${kept()} ` +
        "GROUP BY i.country ORDER BY count DESC",
    ).bind(month),
    env.DB.prepare(
      `SELECT i.version, count(*) AS count FROM installs i WHERE i.last_seen >= ?1 AND ${kept()} GROUP BY i.version`,
    ).bind(week),
    env.DB.prepare("SELECT json FROM release WHERE one = 1"),
  ]);
  const days = new Map();
  for (const { day: d, metric, value } of history.results) {
    days.set(d, { ...(days.get(d) ?? { day: d }), [metric]: value });
  }
  let latest = null;
  try {
    latest = JSON.parse(release.results[0]?.json ?? "null")?.tag_name ?? null;
  } catch {
    // no release kept yet
  }
  return Response.json(
    {
      today,
      latest,
      users: users.results[0],
      history: [...days.values()],
      systems: systems.results,
      countries: countries.results,
      versions: versions.results,
    },
    { headers: { "Cache-Control": "no-store" } },
  );
}

/** The request carries the key, compared through hashes so that the time taken says nothing about it. */
async function allowed(request, env) {
  const given = /^Bearer (.+)$/.exec(request.headers.get("Authorization") ?? "")?.[1];
  if (!env.STATS_KEY || !given) {
    return false;
  }
  const hash = async (text) => new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  const [a, b] = await Promise.all([hash(env.STATS_KEY), hash(given)]);
  let difference = 0;
  for (let i = 0; i < a.length; i++) {
    difference |= a[i] ^ b[i];
  }
  return difference === 0;
}

export function page() {
  return new Response(PAGE, {
    headers: {
      "Content-Type": "text/html; charset=utf-8",
      "Cache-Control": "no-store",
      "Content-Security-Policy":
        "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; " +
        "base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
      "Referrer-Policy": "no-referrer",
      "X-Robots-Tag": "noindex",
    },
  });
}

const PAGE = `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>BatteryTray users</title>
<style>
  :root {
    color-scheme: light;
    --surface: #ffffff; --card: #f6f6f7; --line: #e4e4e7; --grid: #ececee;
    --text: #191919; --text-2: #666666;
    --s1: #257de0; --s2: #f47230; --s3: #00b894; --bar: #257de0;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      color-scheme: dark;
      --surface: #111214; --card: #1b1c1f; --line: #2b2c30; --grid: #2a2b2f;
      --text: #efefef; --text-2: #9c9c9c;
      --s1: #338ced; --s2: #e76624; --s3: #00a888; --bar: #338ced;
    }
  }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--surface); color: var(--text);
         font: 15px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; }
  main { max-width: 1100px; margin: 0 auto; padding: 28px 16px 56px; }
  header { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 8px 16px;
           margin-bottom: 20px; }
  h1 { font-size: 22px; margin: 0; }
  h2 { font-size: 15px; margin: 0 0 12px; }
  .muted { color: var(--text-2); font-size: 13px; }
  .tiles { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; margin-bottom: 16px; }
  .tile, .card { background: var(--card); border: 1px solid var(--line); border-radius: 12px; }
  .tile { padding: 14px 16px; }
  .tile .value { font-size: 28px; font-weight: 600; margin-top: 2px; }
  .card { padding: 16px; margin-bottom: 16px; }
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 16px; }
  .cards .card { margin-bottom: 0; }
  .legend { display: flex; flex-wrap: wrap; gap: 6px 16px; margin-bottom: 8px; font-size: 13px; color: var(--text-2); }
  .key { display: inline-block; width: 14px; height: 2px; border-radius: 1px; vertical-align: middle; margin-right: 6px; }
  .chart { position: relative; }
  .chart svg { display: block; width: 100%; height: 260px; overflow: visible; }
  .tip { position: absolute; pointer-events: none; background: var(--surface); border: 1px solid var(--line);
         border-radius: 8px; padding: 8px 10px; font-size: 13px; white-space: nowrap; display: none;
         box-shadow: 0 4px 16px rgba(0,0,0,.18); }
  .tip b { font-weight: 600; font-variant-numeric: tabular-nums; }
  .rows { display: grid; gap: 8px; }
  .row { display: grid; grid-template-columns: minmax(90px, 38%) 1fr; gap: 10px; align-items: center; font-size: 13px; }
  .row .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row .track { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .row .fill { height: 14px; border-radius: 0 4px 4px 0; background: var(--bar); min-width: 2px; }
  .row .num { color: var(--text-2); font-variant-numeric: tabular-nums; white-space: nowrap; }
  details { margin-top: 10px; font-size: 13px; }
  table { border-collapse: collapse; margin-top: 8px; font-variant-numeric: tabular-nums; }
  th, td { padding: 3px 12px 3px 0; text-align: right; border-bottom: 1px solid var(--line); }
  th:first-child, td:first-child { text-align: left; }
  form { display: flex; gap: 8px; flex-wrap: wrap; }
  input { font: inherit; padding: 8px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--card);
          color: var(--text); min-width: 260px; }
  button { font: inherit; padding: 8px 14px; border-radius: 8px; border: 0; background: var(--s1); color: #fff; }
  .empty { color: var(--text-2); font-size: 13px; padding: 24px 0; text-align: center; }
</style>
</head>
<body>
<main>
  <header><h1 id="title"></h1><span class="muted" id="when"></span></header>
  <div id="login" hidden>
    <p id="ask"></p>
    <form id="form"><input id="key" type="password" autocomplete="current-password"><button id="go"></button></form>
  </div>
  <div id="stats" hidden>
    <div class="tiles" id="tiles"></div>
    <p class="muted" id="pending"></p>
    <div class="card">
      <h2 id="h-history"></h2>
      <div class="legend" id="legend"></div>
      <div class="chart" id="chart"></div>
      <details><summary id="h-table"></summary><div id="table"></div></details>
    </div>
    <div class="cards">
      <div class="card"><h2 id="h-countries"></h2><div class="rows" id="countries"></div></div>
      <div class="card"><h2 id="h-systems"></h2><div class="rows" id="systems"></div></div>
      <div class="card"><h2 id="h-versions"></h2><div class="rows" id="versions"></div></div>
    </div>
  </div>
</main>
<script>
"use strict";
var lang = /^ru\\b/i.test(navigator.language) ? "ru" : "en";
var T = {
  ru: { title: "Пользователи BatteryTray", ask: "Ключ от статистики:", go: "Открыть", wrong: "Ключ не подошёл.",
        day: "Сегодня", week: "За 7 дней", month: "За 30 дней", installs: "Всего копий", fresh: "Новых за 7 дней",
        latest: "На последней версии", history: "Пользователи по дням", table: "Таблица",
        users: "За день", users7: "За 7 дней", users30: "За 30 дней", added: "Новых",
        countries: "Страны, за 30 дней", systems: "Системы, за 30 дней", versions: "Версии, за 7 дней",
        nohistory: "График появится после первых полных суток.", none: "Пока никого.", unknown: "Не определена",
        updated: "обновлено", date: "Дата",
        pending: "Не считаются: {0} — копии этой недели, которые проверили обновления только раз или в пределах трёх часов (CI, пробные запуски, тесты). Пользователем копия становится, когда её проверки разделяют хотя бы три часа." },
  en: { title: "BatteryTray users", ask: "The statistics key:", go: "Open", wrong: "That key doesn't fit.",
        day: "Today", week: "Last 7 days", month: "Last 30 days", installs: "All copies", fresh: "New, last 7 days",
        latest: "On the latest version", history: "Users by day", table: "Table",
        users: "Of the day", users7: "Of 7 days", users30: "Of 30 days", added: "New",
        countries: "Countries, last 30 days", systems: "Systems, last 30 days", versions: "Versions, last 7 days",
        nohistory: "The chart starts after the first full day.", none: "No one yet.", unknown: "Unknown",
        updated: "updated", date: "Date",
        pending: "Not counted: {0} - copies of this week that checked for updates only once or within three hours (CI, quick tries, tests). A copy becomes a user once its checks are at least three hours apart." }
}[lang];
var number = new Intl.NumberFormat(lang);
var percent = new Intl.NumberFormat(lang, { style: "percent", maximumFractionDigits: 0 });
var regions = Intl.DisplayNames ? new Intl.DisplayNames([lang], { type: "region" }) : null;
var SERIES = [["users", "--s1"], ["users7", "--s2"], ["users30", "--s3"]];
var $ = function (id) { return document.getElementById(id); };
function text(id, value) { $(id).textContent = value; }
function el(tag, cls, content) {
  var e = document.createElement(tag);
  if (cls) e.className = cls;
  if (content !== undefined) e.textContent = content;
  return e;
}
function share(part, whole) {
  return part && part / whole < 0.005 ? "<" + percent.format(0.01) : percent.format(part / whole);
}
function css(name) { return getComputedStyle(document.documentElement).getPropertyValue(name).trim(); }
function date(d, long) {
  return new Intl.DateTimeFormat(lang, long ? { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" }
    : { day: "numeric", month: "short", timeZone: "UTC" }).format(new Date(d + "T00:00:00Z"));
}

document.title = T.title;
text("title", T.title); text("ask", T.ask); text("go", T.go);
["history", "table", "countries", "systems", "versions"].forEach(function (k) { text("h-" + k, T[k]); });

function key() {
  var m = /[#&]key=([^&]+)/.exec(location.hash);
  if (m) {
    try { localStorage.setItem("stats-key", decodeURIComponent(m[1])); } catch (e) {}
    history.replaceState(null, "", location.pathname);
    return decodeURIComponent(m[1]);
  }
  try { return localStorage.getItem("stats-key"); } catch (e) { return null; }
}

function load(k) {
  if (!k) { $("login").hidden = false; return; }
  fetch("/stats/data", { headers: { Authorization: "Bearer " + k }, cache: "no-store" }).then(function (r) {
    if (r.status === 403) {
      try { localStorage.removeItem("stats-key"); } catch (e) {}
      $("login").hidden = false; text("ask", T.wrong + " " + T.ask);
      return null;
    }
    return r.json();
  }).then(function (data) { if (data) show(data); });
}
$("form").addEventListener("submit", function (e) {
  e.preventDefault();
  var k = $("key").value.trim();
  try { localStorage.setItem("stats-key", k); } catch (err) {}
  $("login").hidden = true;
  load(k);
});

function show(data) {
  $("stats").hidden = false;
  text("when", T.updated + " " + new Date().toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" }));
  var u = data.users;
  var onLatest = 0;
  var latest = (data.latest || "").replace(/^v/i, "");
  data.versions.forEach(function (v) { if (v.version === latest) onLatest += v.count; });
  var tiles = [[T.day, number.format(u.day)], [T.week, number.format(u.week)], [T.month, number.format(u.month)],
    [T.installs, number.format(u.installs)], [T.fresh, number.format(u.new_week)],
    [T.latest, u.week ? share(onLatest, u.week) : "—", latest]];
  var box = $("tiles");
  box.textContent = "";
  tiles.forEach(function (t) {
    var tile = el("div", "tile");
    tile.appendChild(el("div", "muted", t[0]));
    tile.appendChild(el("div", "value", t[1]));
    if (t[2]) tile.appendChild(el("div", "muted", t[2]));
    box.appendChild(tile);
  });
  text("pending", u.pending ? T.pending.replace("{0}", number.format(u.pending)) : "");
  chart(data.history);
  rows("countries", data.countries.map(function (c) {
    var name = c.country === "XX" ? T.unknown : c.country === "T1" ? "Tor" : regions ? regions.of(c.country) : c.country;
    return [name, c.count];
  }), u.month);
  var systems = {};
  data.systems.forEach(function (s) {
    var name;
    if (s.os === "Windows") name = Number((s.os_version.split(".")[2]) || 0) >= 22000 ? "Windows 11" : "Windows 10";
    else name = "macOS " + s.os_version.split(".")[0];
    systems[name] = (systems[name] || 0) + s.count;
  });
  rows("systems", Object.keys(systems).map(function (k) { return [k, systems[k]]; }), u.month);
  rows("versions", data.versions.map(function (v) { return [v.version, v.count]; }), u.week, true);
}

/** Horizontal bars, the longest first (or the newest version first), each with its count and share. */
function rows(id, list, total, byVersion) {
  var box = $(id);
  box.textContent = "";
  if (!list.length) { box.appendChild(el("div", "empty", T.none)); return; }
  list.sort(byVersion ? function (a, b) {
    var x = a[0].split(".").map(Number), y = b[0].split(".").map(Number);
    for (var i = 0; i < 3; i++) if (x[i] !== y[i]) return y[i] - x[i];
    return 0;
  } : function (a, b) { return b[1] - a[1]; });
  var max = Math.max.apply(null, list.map(function (r) { return r[1]; }));
  list.forEach(function (r) {
    var row = el("div", "row");
    var name = el("div", "name", r[0]);
    name.title = r[0];
    row.appendChild(name);
    var track = el("div", "track");
    var fill = el("div", "fill");
    fill.style.width = "calc((100% - 92px) * " + (r[1] / max) + ")";
    track.appendChild(fill);
    track.appendChild(el("span", "num", number.format(r[1]) + (total ? " · " + share(r[1], total) : "")));
    row.appendChild(track);
    box.appendChild(row);
  });
}

/** Users of each day, of 7 and of 30 days: three lines on one axis, a crosshair with every value, a table. */
function chart(history) {
  var box = $("chart"), legend = $("legend");
  box.textContent = ""; legend.textContent = "";
  var table = $("table");
  table.textContent = "";
  if (!history.length) { box.appendChild(el("div", "empty", T.nohistory)); return; }
  SERIES.forEach(function (s) {
    var item = el("span");
    var keyLine = el("span", "key");
    keyLine.style.background = css(s[1]);
    item.appendChild(keyLine);
    item.appendChild(document.createTextNode(T[s[0]]));
    legend.appendChild(item);
  });
  var W = box.clientWidth || 800, H = 260, left = 44, right = 76, top = 10, bottom = 26;
  var max = 1;
  history.forEach(function (d) { SERIES.forEach(function (s) { max = Math.max(max, d[s[0]] || 0); }); });
  var step = Math.pow(10, Math.floor(Math.log10(max))), nice = [1, 2, 2.5, 5, 10];
  var tick = step;
  for (var i = 0; i < nice.length; i++) { if (max / (nice[i] * step) <= 4) { tick = nice[i] * step; break; } }
  var yMax = Math.ceil(max / tick) * tick;
  var n = history.length;
  var x = function (i) { return left + (n === 1 ? (W - left - right) / 2 : i * (W - left - right) / (n - 1)); };
  var y = function (v) { return top + (H - top - bottom) * (1 - v / yMax); };
  var NS = "http://www.w3.org/2000/svg";
  var svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 " + W + " " + H);
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", T.history);
  function add(tag, attrs, content) {
    var e = document.createElementNS(NS, tag);
    for (var a in attrs) e.setAttribute(a, attrs[a]);
    if (content !== undefined) e.textContent = content;
    svg.appendChild(e);
    return e;
  }
  for (var v = 0; v <= yMax; v += tick) {
    add("line", { x1: left, x2: W - right, y1: y(v), y2: y(v), stroke: css("--grid"), "stroke-width": 1 });
    add("text", { x: left - 8, y: y(v) + 4, "text-anchor": "end", "font-size": 11, fill: css("--text-2") },
      number.format(v));
  }
  var labels = Math.min(n, Math.max(2, Math.floor((W - left - right) / 90)));
  for (var j = 0; j < labels; j++) {
    var at = labels === 1 ? 0 : Math.round(j * (n - 1) / (labels - 1));
    add("text", { x: x(at), y: H - 6, "text-anchor": "middle", "font-size": 11, fill: css("--text-2") },
      date(history[at].day));
  }
  var ends = [];
  SERIES.forEach(function (s) {
    var points = history.map(function (d, i) { return x(i) + "," + y(d[s[0]] || 0); }).join(" ");
    add("polyline", { points: points, fill: "none", stroke: css(s[1]), "stroke-width": 2,
      "stroke-linejoin": "round", "stroke-linecap": "round" });
    var last = history[n - 1][s[0]] || 0;
    add("circle", { cx: x(n - 1), cy: y(last), r: 4, fill: css(s[1]) });
    ends.push({ y: y(last), name: T[s[0]] });
  });
  ends.sort(function (a, b) { return a.y - b.y; });
  for (var k = 1; k < ends.length; k++) ends[k].y = Math.max(ends[k].y, ends[k - 1].y + 14);
  ends.forEach(function (e) {
    add("text", { x: W - right + 10, y: e.y + 4, "font-size": 12, fill: css("--text-2") }, e.name);
  });
  var cross = add("line", { y1: top, y2: H - bottom, stroke: css("--text-2"), "stroke-width": 1, visibility: "hidden" });
  box.appendChild(svg);
  var tip = el("div", "tip");
  box.appendChild(tip);
  svg.addEventListener("pointermove", function (e) {
    var r = svg.getBoundingClientRect();
    var px = (e.clientX - r.left) * W / r.width;
    var i = Math.max(0, Math.min(n - 1, Math.round(n === 1 ? 0 : (px - left) / ((W - left - right) / (n - 1)))));
    cross.setAttribute("x1", x(i)); cross.setAttribute("x2", x(i)); cross.setAttribute("visibility", "visible");
    tip.textContent = "";
    tip.appendChild(el("div", "muted", date(history[i].day, true)));
    SERIES.concat([["new", "--text-2"]]).forEach(function (s) {
      var line = el("div");
      var keyLine = el("span", "key");
      keyLine.style.background = css(s[1]);
      line.appendChild(keyLine);
      line.appendChild(el("b", null, number.format(history[i][s[0]] || 0)));
      line.appendChild(document.createTextNode(" " + (s[0] === "new" ? T.added : T[s[0]])));
      tip.appendChild(line);
    });
    tip.style.display = "block";
    var tx = x(i) * r.width / W + 12;
    if (tx + tip.offsetWidth > r.width) tx = x(i) * r.width / W - tip.offsetWidth - 12;
    tip.style.left = tx + "px";
    tip.style.top = "8px";
  });
  svg.addEventListener("pointerleave", function () {
    tip.style.display = "none"; cross.setAttribute("visibility", "hidden");
  });
  var t = el("table");
  var head = el("tr");
  [T.date, T.users, T.users7, T.users30, T.added].forEach(function (h) { head.appendChild(el("th", null, h)); });
  t.appendChild(head);
  history.slice().reverse().forEach(function (d) {
    var tr = el("tr");
    tr.appendChild(el("td", null, date(d.day, true)));
    ["users", "users7", "users30", "new"].forEach(function (m) { tr.appendChild(el("td", null, number.format(d[m] || 0))); });
    t.appendChild(tr);
  });
  table.appendChild(t);
}

load(key());
</script>
</body>
</html>
`;
