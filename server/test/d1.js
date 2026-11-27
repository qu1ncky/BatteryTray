// D1 for the tests: the same SQLite, in memory, with the migrations applied, behind the part of D1's API the server
// uses (prepare/bind/first/all/run, batch in one transaction).

import { readdirSync, readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";

export function database() {
  const db = new DatabaseSync(":memory:");
  const folder = new URL("../migrations/", import.meta.url);
  for (const file of readdirSync(folder).sort()) {
    db.exec(readFileSync(new URL(file, folder), "utf8"));
  }
  const reads = (sql) => /^\s*SELECT\b/i.test(sql);
  const execute = (sql, params) =>
    reads(sql) ? { results: db.prepare(sql).all(...params) } : { results: [], meta: db.prepare(sql).run(...params) };
  const statement = (sql, params = []) => ({
    sql,
    params,
    bind: (...values) => statement(sql, values),
    first: async () => db.prepare(sql).get(...params) ?? null,
    all: async () => ({ results: db.prepare(sql).all(...params) }),
    run: async () => execute(sql, params),
  });
  return {
    sqlite: db,
    broken: false,
    prepare(sql) {
      if (this.broken) {
        throw new Error("D1 is down");
      }
      return statement(sql);
    },
    async batch(statements) {
      db.exec("BEGIN");
      try {
        const results = statements.map((s) => execute(s.sql, s.params));
        db.exec("COMMIT");
        return results;
      } catch (e) {
        db.exec("ROLLBACK");
        throw e;
      }
    },
  };
}
