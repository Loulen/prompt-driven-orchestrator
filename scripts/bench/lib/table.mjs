// The markdown of `make bench-gates`: one column per measured ref.

/** One markdown table row, `|` in a cell escaped. */
export const mdRow = (cells) => `| ${cells.map((c) => String(c).replace(/\|/g, "\\|")).join(" | ")} |`;

/** `42 s`, `3 min 07 s`, `1 h 02 min`. */
export function formatDuration(seconds) {
  const s = Math.round(seconds);
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, "0")} s`;
  return `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")} min`;
}

/** One cell: the duration and its outcome, with the machine load when the measure started. */
export function cell(result) {
  if (!result) return "-";
  if (result.na) return result.na;
  if (result.skipped) return `skipped: ${result.skipped}`;
  const load = result.load === undefined ? "" : `, load ${result.load.toFixed(1)}`;
  const note = result.note ? `, ${result.note}` : "";
  const command = result.command ? `\`${result.command}\`: ` : "";
  return `${command}${formatDuration(result.seconds)} (${result.outcome}${note}${load})`;
}

/**
 * The comparison table of several benches (`{ ref, sha, measures: [{ id, label, ...result }] }`):
 * one row per measure, in the order of the first bench, one column per ref.
 */
export function renderTable(benches) {
  const ids = [];
  const labels = new Map();
  for (const bench of benches) {
    for (const m of bench.measures) {
      if (!labels.has(m.id)) ids.push(m.id);
      labels.set(m.id, labels.get(m.id) ?? m.label);
    }
  }
  const head = ["Measure", ...benches.map((b) => `\`${b.ref}\` (${b.sha.slice(0, 8)})`)];
  const rows = ids.map((id) => [labels.get(id), ...benches.map((b) => cell(b.measures.find((m) => m.id === id)))]);
  return [mdRow(head), mdRow(head.map(() => "---")), ...rows.map(mdRow)].join("\n");
}

/** The lines under the table: the machine, sccache, and when each bench ran. */
export function renderNotes(benches, machine) {
  const lines = [`Machine: ${machine.cpus} CPUs, ${machine.os}; sccache: ${machine.sccache ?? "absent"}.`];
  for (const b of benches) {
    lines.push(`\`${b.ref}\` (${b.sha.slice(0, 8)}): ${b.startedAt} -> ${b.finishedAt}, load ${b.loadStart.toFixed(1)} -> ${b.loadEnd.toFixed(1)}.`);
  }
  lines.push("Load: the 1-minute load average when the measure started (other Runs compile on the same machine).");
  return lines.join("\n");
}
