/**
 * S3 — the modules the front shares with the extensions' UI bundles through
 * its import map (ADR-0081): one copy of each, the front's own.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import * as Y from "yjs";
import { describe, expect, it } from "vitest";
import { shareWithExtensions } from "./shared";

const FRONTEND = join(__dirname, "..", "..", "..");

/** The import map of `index.html`: bare specifier → address. */
function importMap(): Record<string, string> {
  const html = readFileSync(join(FRONTEND, "index.html"), "utf8");
  const json = /<script type="importmap">([\s\S]*?)<\/script>/.exec(html)?.[1] ?? "{}";
  return (JSON.parse(json) as { imports: Record<string, string> }).imports;
}

describe("yjs, shared with the extensions", () => {
  it("is mapped to its shim by the import map", () => {
    expect(importMap().yjs).toBe("/shared/yjs.js");
  });

  it("is the front's own copy, every export of it through the shim", async () => {
    shareWithExtensions();
    expect(globalThis.__PDO_SHARED__?.yjs).toBe(Y);
    const shim = (await import("../../../public/shared/yjs.js")) as Record<string, unknown>;
    expect(Object.keys(shim).sort()).toEqual(Object.keys(Y).sort());
    for (const name of Object.keys(Y)) {
      expect(shim[name], name).toBe(Y[name as keyof typeof Y]);
    }
  });
});
