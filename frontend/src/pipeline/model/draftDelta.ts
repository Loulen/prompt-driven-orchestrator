/**
 * The delta between two JSON states of a shared draft (ADR-0088), applied to
 * its replicated document field by field: two people who change two fields of
 * the same node both keep their change.
 *
 * An array element is found by its identity rather than its position: its
 * `id` when it is an object carrying one (nodes, notes, loop regions), its
 * whole JSON otherwise (edges, ports). The daemon applies the same delta
 * (`crates/pdo-pipeline/src/domain/draft/delta.rs`).
 */

import * as Y from "yjs";

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type JsonObject = { [key: string]: Json };

function isObject(value: Json | undefined): value is JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** `value` as JSON with the keys of every object sorted. */
export function canonical(value: Json | undefined): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (isObject(value)) {
    const keys = Object.keys(value).sort();
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canonical(value[k])}`).join(",")}}`;
  }
  return JSON.stringify(value ?? null);
}

/** The identity of an array element. */
export function elementKey(value: Json | undefined): string {
  if (isObject(value) && typeof value.id === "string") return `id:${value.id}`;
  return `json:${canonical(value)}`;
}

function equal(a: Json | undefined, b: Json | undefined): boolean {
  return canonical(a) === canonical(b);
}

/** A JSON value as the document holds it: an object is a map, an array an array. */
export function toY(value: Json): unknown {
  if (Array.isArray(value)) {
    const array = new Y.Array<unknown>();
    array.push(value.map(toY));
    return array;
  }
  if (isObject(value)) {
    const map = new Y.Map<unknown>();
    for (const [key, field] of Object.entries(value)) map.set(key, toY(field));
    return map;
  }
  return value;
}

function jsonOf(value: unknown): Json {
  if (value instanceof Y.AbstractType) return value.toJSON() as Json;
  return (value ?? null) as Json;
}

/** Change `map` from the object `before` to `after`, field by field. */
export function applyObject(map: Y.Map<unknown>, before: JsonObject, after: JsonObject): void {
  const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort();
  for (const key of keys) {
    const old = before[key];
    const next = after[key];
    if (equal(old, next)) continue;
    if (next === undefined) {
      map.delete(key);
      continue;
    }
    const current = map.get(key);
    if (isObject(old) && isObject(next) && current instanceof Y.Map) {
      applyObject(current, old, next);
    } else if (Array.isArray(old) && Array.isArray(next) && current instanceof Y.Array) {
      applyArray(current, old, next);
    } else {
      map.set(key, toY(next));
    }
  }
}

type Step = { keep: [number, number] } | { del: number } | { ins: number };

/** The longest common subsequence of two key lists, as steps. */
function align(old: string[], next: string[]): Step[] {
  const n = old.length;
  const m = next.length;
  const table = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      table[i][j] = old[i] === next[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
    }
  }
  const steps: Step[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (old[i] === next[j]) steps.push({ keep: [i++, j++] });
    else if (table[i + 1][j] >= table[i][j + 1]) steps.push({ del: i++ });
    else steps.push({ ins: j++ });
  }
  while (i < n) steps.push({ del: i++ });
  while (j < m) steps.push({ ins: j++ });
  return steps;
}

function indexOf(array: Y.Array<unknown>, key: string): number {
  const items = array.toArray();
  for (let i = 0; i < items.length; i++) {
    if (elementKey(jsonOf(items[i])) === key) return i;
  }
  return -1;
}

/** Make the element at `index` of `array` go from `before` to `after`. */
function patchAt(array: Y.Array<unknown>, index: number, before: Json, after: Json): void {
  const current = array.get(index);
  if (isObject(before) && isObject(after) && current instanceof Y.Map) {
    applyObject(current, before, after);
  } else if (Array.isArray(before) && Array.isArray(after) && current instanceof Y.Array) {
    applyArray(current, before, after);
  } else {
    array.delete(index, 1);
    array.insert(index, [toY(after)]);
  }
}

function insertAfter(array: Y.Array<unknown>, after: string | null, value: Json): void {
  const existing = indexOf(array, elementKey(value));
  if (existing >= 0) {
    patchAt(array, existing, jsonOf(array.get(existing)), value);
    return;
  }
  const previous = after == null ? -1 : indexOf(array, after);
  const at = after == null ? 0 : previous >= 0 ? previous + 1 : array.length;
  array.insert(at, [toY(value)]);
}

/** Change `array` from `before` to `after`, each element found by identity. */
export function applyArray(array: Y.Array<unknown>, before: Json[], after: Json[]): void {
  const oldKeys = before.map(elementKey);
  const newKeys = after.map(elementKey);
  const steps = align(oldKeys, newKeys);
  const previousKey = (j: number) => (j > 0 ? newKeys[j - 1] : null);
  let s = 0;
  while (s < steps.length) {
    const step = steps[s];
    if ("keep" in step) {
      const [o, n] = step.keep;
      if (!equal(before[o], after[n])) {
        const at = indexOf(array, oldKeys[o]);
        if (at >= 0) patchAt(array, at, before[o], after[n]);
      }
      s++;
      continue;
    }
    // A run of deletions and insertions between two kept elements.
    const deleted: number[] = [];
    const inserted: number[] = [];
    while (s < steps.length && !("keep" in steps[s])) {
      const run = steps[s];
      if ("del" in run) deleted.push(run.del);
      else if ("ins" in run) inserted.push(run.ins);
      s++;
    }
    // An element without `id` replaced by another one at the same place is
    // the same element edited (an edge whose condition changed).
    let paired = 0;
    while (paired < Math.min(deleted.length, inserted.length)) {
      const o = deleted[paired];
      const n = inserted[paired];
      const plain = !oldKeys[o].startsWith("id:") && !newKeys[n].startsWith("id:");
      if (!(plain && isObject(before[o]) && isObject(after[n]))) break;
      const at = indexOf(array, oldKeys[o]);
      if (at >= 0) patchAt(array, at, before[o], after[n]);
      paired++;
    }
    for (const o of deleted.slice(paired)) {
      const at = indexOf(array, oldKeys[o]);
      if (at >= 0) array.delete(at, 1);
    }
    for (const n of inserted.slice(paired)) insertAfter(array, previousKey(n), after[n]);
  }
}

/** The span `[index, index + remove)` of `before` replaced by `insert`
 *  gives `after` (UTF-16 code units); `null` when they are equal. */
export function diffText(before: string, after: string): { index: number; remove: number; insert: string } | null {
  if (before === after) return null;
  let prefix = 0;
  while (prefix < before.length && prefix < after.length && before[prefix] === after[prefix]) prefix++;
  let suffix = 0;
  while (
    suffix < before.length - prefix &&
    suffix < after.length - prefix &&
    before[before.length - 1 - suffix] === after[after.length - 1 - suffix]
  ) {
    suffix++;
  }
  // Never split a surrogate pair.
  const low = (s: string, i: number) => i < s.length && s.charCodeAt(i) >= 0xdc00 && s.charCodeAt(i) <= 0xdfff;
  while (prefix > 0 && low(before, prefix)) prefix--;
  while (suffix > 0 && low(before, before.length - suffix)) suffix--;
  return {
    index: prefix,
    remove: before.length - suffix - prefix,
    insert: after.slice(prefix, after.length - suffix),
  };
}

/** Where `caret` moves when `before` becomes `after` around it. */
export function shiftCaret(caret: number, before: string, after: string): number {
  const edit = diffText(before, after);
  if (!edit || caret <= edit.index) return caret;
  if (caret <= edit.index + edit.remove) return edit.index + edit.insert.length;
  return caret + edit.insert.length - edit.remove;
}

/** Change the prompts `map` (one text per node) from `before` to `after`. */
export function applyPrompts(
  map: Y.Map<unknown>,
  before: Record<string, string>,
  after: Record<string, string>,
): void {
  for (const node of Object.keys(before)) {
    if (!(node in after)) map.delete(node);
  }
  for (const [node, next] of Object.entries(after)) {
    const text = map.get(node);
    if (!(text instanceof Y.Text)) {
      map.set(node, new Y.Text(next));
      continue;
    }
    const edit = diffText(before[node] ?? "", next);
    if (!edit) continue;
    const length = text.length;
    const index = Math.min(edit.index, length);
    text.delete(index, Math.min(edit.remove, length - index));
    text.insert(index, edit.insert);
  }
}

/** The prompts as the document holds them. */
export function promptsOf(map: Y.Map<unknown>): Record<string, string> {
  const prompts: Record<string, string> = {};
  map.forEach((value, node) => {
    if (value instanceof Y.Text) prompts[node] = value.toString();
    else if (typeof value === "string") prompts[node] = value;
  });
  return prompts;
}
