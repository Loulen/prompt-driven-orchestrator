/**
 * The guard of the Opérations en cours: every mutation the front sends
 * (POST, PUT, PATCH, DELETE through `request`) names itself with a `label`
 * a person reads (« Archiving run »), or says it is `silent` (a POST that
 * only reads). A mutation without one falls back to « Saving… »; a label
 * that is a route (`POST /runs`) says nothing to a person either. The
 * count of those must not go up.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

/** Mutations allowed without an explicit label. Lower it when you label one; never raise it. */
const BASELINE = 0;

const SRC = join(import.meta.dirname, "..", "..");

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return entry === "generated" ? [] : walk(path);
    return /\.tsx?$/.test(path) && !path.includes(".test.") ? [path] : [];
  });
}

/** The text of the arguments of the call whose `(` is at `open`, up to its `)`. */
function argumentsAt(text: string, open: number): string {
  let depth = 0;
  let quote: string | null = null;
  for (let i = open; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === "\\") i++;
      else if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") quote = c;
    else if (c === "(") depth++;
    else if (c === ")" && --depth === 0) return text.slice(open + 1, i);
  }
  return text.slice(open + 1);
}

const MUTATION = /^\s*"(POST|PUT|PATCH|DELETE)"/;
const ROUTE_LABEL = /\blabel:\s*[`"'](GET|POST|PUT|PATCH|DELETE)\b/;

/** `file:line METHOD` of each mutation sent without a label a person reads. */
export function unlabeledMutations(files: { path: string; text: string }[]): string[] {
  const found: string[] = [];
  for (const { path, text } of files) {
    for (const m of text.matchAll(/\brequest(?:<[^>()]*(?:<[^>()]*>[^>()]*)*>)?\(/g)) {
      const open = (m.index ?? 0) + m[0].length - 1;
      const args = argumentsAt(text, open);
      const method = MUTATION.exec(args)?.[1];
      if (!method) continue;
      const silent = /\bsilent:\s*true\b/.test(args) || /\bkeepalive:\s*true\b/.test(args);
      const labelled = /\blabel\b/.test(args) && !ROUTE_LABEL.test(args);
      if (silent || labelled) continue;
      const line = text.slice(0, open).split("\n").length;
      found.push(`${path}:${line} ${method}`);
    }
  }
  return found;
}

describe("the mutations of the front name themselves", () => {
  it("finds a mutation without a label, or with a route for a label", () => {
    const files = [
      { path: "a.ts", text: 'request("POST", "/x", { body });\nrequest("DELETE", `/y/${id}`, { label: "DELETE /y" });' },
      { path: "b.ts", text: 'request<Run>(\n  "PUT",\n  "/z",\n  { label: "Saving z" },\n);\nrequest("GET", "/r");' },
      { path: "c.ts", text: 'request("POST", "/authorize", { label: "POST /authorize", silent: true });' },
    ];
    expect(unlabeledMutations(files)).toEqual(["a.ts:1 POST", "a.ts:2 DELETE"]);
  });

  it(`no more than ${BASELINE} mutation of the front lacks a label a person reads`, () => {
    const files = walk(SRC)
      .filter((p) => !p.endsWith(join("shared", "http", "client.ts")))
      .map((p) => ({ path: relative(SRC, p), text: readFileSync(p, "utf8") }));
    const unlabeled = unlabeledMutations(files);
    expect(unlabeled.length, `mutations without a label:\n${unlabeled.join("\n")}`).toBeLessThanOrEqual(BASELINE);
  });
});
