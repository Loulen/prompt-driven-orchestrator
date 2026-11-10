/**
 * The `when` clauses of the contributions (ADR-0081, amendment #29): the core
 * evaluates each on the context of its location, and renders nothing whose
 * clause is false. Keys are dotted paths into the context (`run.status`,
 * `run.attributes.test/started_by`); an absent key reads as nothing.
 */

import type { When } from "../generated/When";

/** The value at the dotted `key` of `context`; `undefined` when absent. */
export function lookup(context: unknown, key: string): unknown {
  let value: unknown = context;
  for (const part of key.split(".")) {
    if (value === null || typeof value !== "object") return undefined;
    value = (value as Record<string, unknown>)[part];
  }
  return value;
}

function holdsSomething(value: unknown): boolean {
  return value !== undefined && value !== null && value !== false && value !== "";
}

/** Whether `when` holds on `context`; always without a clause. An operator
 *  this front does not know is false: a newer extension's element stays hidden
 *  rather than showing where it should not. */
export function evaluateWhen(when: When | null | undefined, context: unknown): boolean {
  if (!when) return true;
  if ("has" in when) return holdsSomething(lookup(context, when.has));
  if ("eq" in when) {
    const value = lookup(context, when.eq[0]);
    return value !== undefined && value !== null && String(value) === when.eq[1];
  }
  if ("not" in when) return !evaluateWhen(when.not, context);
  if ("all" in when) return when.all.every((inner) => evaluateWhen(inner, context));
  if ("any" in when) return when.any.some((inner) => evaluateWhen(inner, context));
  return false;
}
