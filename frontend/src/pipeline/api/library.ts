/**
 * The node library over HTTP (`~/.pdo/library`), a pasted node's parse, and
 * the import of a Claude Code workflow.
 */

import { request } from "../../shared/http/client";
import type {
  InstantiateResult,
  LibraryEntry,
  LibrarySaveSpec,
  ParseNodeResult,
} from "../model/library";

export function fetchLibrary(): Promise<LibraryEntry[]> {
  return request<LibraryEntry[]>("GET", "/library");
}

export function saveToLibrary(spec: LibrarySaveSpec): Promise<LibraryEntry> {
  return request<LibraryEntry>("POST", "/library", { body: spec, label: "POST /library" });
}

export function deleteFromLibrary(name: string): Promise<void> {
  return request<void>(
    "DELETE",
    `/library/${encodeURIComponent(name)}`,
    { responseMode: "void", label: `DELETE /library/${name}` },
  );
}

export function instantiateFromLibrary(name: string): Promise<InstantiateResult> {
  return request<InstantiateResult>(
    "POST",
    `/library/${encodeURIComponent(name)}/instantiate`,
    { label: `POST /library/${name}/instantiate` },
  );
}

/**
 * Validate a single node's YAML on the daemon (#345 / ADR-0016) and get back a
 * canvas-instantiable spec. The front holds no YAML parser: it POSTs the raw
 * text (paste OR uploaded `.yaml`) and the daemon parses with the same serde
 * structs the pipeline parser uses. Mirror of {@link importWorkflow}: a 400
 * body carries a verbatim `error`; a 200 carries `{spec, prompt, warnings}`.
 */
export function parseNodeYaml(yaml: string): Promise<ParseNodeResult> {
  return request<ParseNodeResult>("POST", "/nodes/parse", { body: { yaml }, label: "POST /nodes/parse" });
}

/// Import a Claude Code workflow `.js` as a draft library pipeline (#155). The
/// `content` is the raw file text (read client-side via `File.text()` — the
/// daemon never reads `~/.claude/workflows` off disk). `filename` seeds the
/// fallback pipeline name. Returns the new id, scope, and any lossy-translation
/// warnings; a 400 body carries a verbatim `error` a real `.js` can trigger.
export function importWorkflow(
  filename: string,
  content: string,
): Promise<{ id: string; scope: string; warnings?: string[] }> {
  return request<{ id: string; scope: string; warnings?: string[] }>(
    "POST",
    "/library/import",
    { body: { filename, content }, label: "POST /library/import" },
  );
}
