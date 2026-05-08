export const PREDICATES = ["eq", "neq", "lt", "lte", "gt", "gte", "in", "not_in"] as const;

export const PREDICATE_LABELS: Record<string, string> = {
  eq: "=", neq: "!=", lt: "<", lte: "<=", gt: ">", gte: ">=",
  in: "in", not_in: "not in",
};

export function formatWhenClause(when: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const [field, predicate] of Object.entries(when)) {
    if (field === "any") continue;
    if (typeof predicate === "object" && predicate !== null) {
      for (const [op, val] of Object.entries(predicate as Record<string, unknown>)) {
        const symbol = PREDICATE_LABELS[op] ?? op;
        const valStr = Array.isArray(val) ? `[${val.join(", ")}]` : String(val);
        parts.push(`${field} ${symbol} ${valStr}`);
      }
    }
  }
  return parts.join(" & ");
}
