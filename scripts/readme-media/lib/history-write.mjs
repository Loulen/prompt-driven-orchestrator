// Put a mocked-history plan (history-plan.mjs) on disk: the transcripts and the
// manual price table under the demo HOME, the events and trigger fires in the
// demo event log. Only ever pointed at a demo instance — the paths come from
// DemoInstance, never from the user's environment.

import fs from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

export function writeHistory(plan, { home, dbPath, triggerId }) {
  for (const file of plan.files) {
    const target = path.join(home, file.path);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, file.content);
  }

  const pricesDir = path.join(home, ".pdo", "prices");
  fs.mkdirSync(pricesDir, { recursive: true });
  const rows = Object.entries(plan.prices).map(([model, p]) => `  ${model}: { input: ${p.input}, output: ${p.output} }`);
  fs.writeFileSync(path.join(pricesDir, "models.yaml"), `# Demo instance only (make readme-media): models the compiled table does not price.\nmodels:\n${rows.join("\n")}\n`);

  const db = new DatabaseSync(dbPath);
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    db.exec("BEGIN");
    const insertEvent = db.prepare("INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES (?, ?, ?, ?, ?, ?)");
    for (const e of plan.events) {
      insertEvent.run(e.run_id, e.ts, e.kind, e.node_id, e.iter, e.payload === null ? null : JSON.stringify(e.payload));
    }
    if (triggerId) {
      const insertFire = db.prepare(
        "INSERT INTO trigger_fires (trigger_id, ts, outcome, reason, run_id, guard_stdout, guard_stderr, guard_exit_code, source) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
      );
      for (const f of plan.fires) {
        insertFire.run(triggerId, f.ts, f.outcome, f.reason, f.run_id, f.guard_stdout, f.guard_stderr, f.guard_exit_code, f.source);
      }
      const last = plan.fires.at(-1);
      if (last) {
        db.prepare("UPDATE triggers SET last_fired_at = ?, last_outcome = ? WHERE id = ?").run(last.ts, last.outcome, triggerId);
      }
    }
    db.exec("COMMIT");
  } catch (error) {
    try {
      db.exec("ROLLBACK");
    } catch {
      // nothing was begun
    }
    throw error;
  } finally {
    db.close();
  }
}
