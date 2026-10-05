/**
 * The question of authorization (CORE-5): would the daemon allow these
 * Operations to the caller? `POST /authorize` answers one decision per
 * Operation, in order, without running any. Without extension, everything is
 * allowed. The UI hides what would be refused; the daemon still refuses it.
 */

import { request } from "../http/client";

/** What an Operation does: `read`, `create`, `update`, `delete`, or a named action. */
export type OperationVerb = "read" | "create" | "update" | "delete" | { action: string };

/** An Operation, in its wire form (`pdo_extension_api::Operation`). */
export interface OperationQuestion {
  /** `run`, `project`, `repository`…, or `<extension>/<kind>`. */
  resource: string;
  verb: OperationVerb;
  target?: string | null;
  /** The Projets it is in; omitted, the daemon places it as it would its own. */
  projects?: string[];
}

export type Decision = { decision: "allow" } | { decision: "deny"; reason: string };

export function askAuthorization(operations: OperationQuestion[]): Promise<Decision[]> {
  if (operations.length === 0) return Promise.resolve([]);
  return request<{ decisions: Decision[] }>("POST", "/authorize", {
    body: { operations },
    label: "POST /authorize",
  }).then((answer) => answer.decisions);
}

export function isAllowed(decision: Decision | undefined): boolean {
  return decision?.decision !== "deny";
}
