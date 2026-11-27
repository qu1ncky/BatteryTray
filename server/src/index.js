// The update server. GET /v1/latest answers an app's update check with the latest release (release.js) and counts
// the copy that asked (count.js); /stats is the owners' page of those numbers (stats.js). Whatever goes wrong here,
// an app that gets no good answer asks GitHub itself (src/updater/mod.rs), so the server can fail or be switched off
// (SERVE in wrangler.jsonc) without anyone missing an update.

import { record, sumUp } from "./count.js";
import { latest } from "./release.js";
import { data, page } from "./stats.js";

export default {
  async fetch(request, env, ctx) {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response(null, { status: 405, headers: { Allow: "GET, HEAD" } });
    }
    switch (new URL(request.url).pathname) {
      case "/v1/latest":
        return check(request, env, ctx);
      case "/stats":
        return page();
      case "/stats/data":
        return data(request, env);
      default:
        return new Response("Not found\n", { status: 404 });
    }
  },

  async scheduled(controller, env, ctx) {
    ctx.waitUntil(
      sumUp(env, controller.scheduledTime).catch((e) =>
        console.error(JSON.stringify({ message: "can't sum the day up", error: String(e) })),
      ),
    );
  },
};

async function check(request, env, ctx) {
  ctx.waitUntil(
    record(request, env).catch((e) => console.error(JSON.stringify({ message: "can't count", error: String(e) }))),
  );
  if (env.SERVE !== "on") {
    return unavailable();
  }
  try {
    return new Response(await latest(env, ctx), {
      headers: { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" },
    });
  } catch {
    return unavailable();
  }
}

/** No release to give: the app asks GitHub. */
function unavailable() {
  return new Response("No release to give, ask GitHub\n", {
    status: 503,
    headers: { "Cache-Control": "no-store", "Retry-After": "300" },
  });
}
