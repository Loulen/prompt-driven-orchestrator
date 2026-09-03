/**
 * Client-side mirror of the daemon's `skill_bank::validate_skill_md` (#668): the
 * paste popup re-parses on every keystroke and lights five checks. The daemon
 * stays authoritative (a 400/409 is rendered exactly like a local failure); this
 * only exists so the operator sees the refusal before pressing Create.
 *
 * The frontmatter parser here is deliberately small: `key: value` lines, quoted
 * scalars, and `key: |` / `key: >` block scalars — enough for a `SKILL.md`. It
 * never claims to be YAML; anything odd is surfaced as `frontmatter` raw text
 * and the daemon has the last word.
 */

export type CheckId = "frontmatter" | "name" | "description" | "body" | "unique";

export type CheckState = "pass" | "fail" | "warn" | "pending";

export interface SkillCheck {
  id: CheckId;
  state: CheckState;
  /** The row label of the popup's check list. */
  label: string;
}

export interface ParsedSkillMd {
  /** `null` when no `---` block opens (and closes) the text. */
  frontmatter: Record<string, string> | null;
  /** Raw YAML text between the fences, for line highlighting. */
  frontmatterText: string;
  body: string;
  name: string | null;
  description: string | null;
}

export interface SkillMdValidation {
  parsed: ParsedSkillMd;
  checks: SkillCheck[];
  /** All checks pass (the `unique` check counts only when `existingNames` is known). */
  valid: boolean;
  /** Reason + consequence, for the callout under the preview card. */
  reason: string | null;
}

export const KEBAB_CASE = /^[a-z0-9]+(-[a-z0-9]+)*$/;

export function isKebabCase(name: string): boolean {
  return KEBAB_CASE.test(name);
}

function unquote(raw: string): string {
  const value = raw.trim();
  if (value.length >= 2) {
    const first = value[0];
    const last = value[value.length - 1];
    if ((first === '"' && last === '"') || (first === "'" && last === "'")) {
      return value.slice(1, -1);
    }
  }
  return value;
}

/** Split `text` into frontmatter YAML and body; `null` when no closed block. */
export function splitFrontmatter(text: string): { yaml: string; body: string } | null {
  const trimmed = text.replace(/^\s+/, "");
  if (!trimmed.startsWith("---")) return null;
  const afterFence = trimmed.slice(3);
  if (!afterFence.startsWith("\n") && !afterFence.startsWith("\r\n")) return null;
  const lines = afterFence.replace(/^\r?\n/, "").split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].replace(/\r$/, "") === "---") {
      return { yaml: lines.slice(0, i).join("\n"), body: lines.slice(i + 1).join("\n") };
    }
  }
  return null;
}

/** Minimal `key: value` parser (top-level keys only, block scalars folded). */
export function parseSimpleYaml(yaml: string): Record<string, string> {
  const out: Record<string, string> = {};
  const lines = yaml.split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i].replace(/\r$/, "");
    i++;
    if (!line.trim() || line.trim().startsWith("#")) continue;
    if (/^\s/.test(line)) continue; // nested content of a key we do not model
    const match = /^([A-Za-z0-9_-]+)\s*:(.*)$/.exec(line);
    if (!match) continue;
    const key = match[1];
    let value = match[2].trim();
    if (value === "|" || value === ">" || value === "|-" || value === ">-") {
      const block: string[] = [];
      while (i < lines.length && (/^\s/.test(lines[i]) || lines[i].trim() === "")) {
        block.push(lines[i].replace(/^\s+/, ""));
        i++;
      }
      value = block.join(value.startsWith("|") ? "\n" : " ").trim();
    } else if (value === "") {
      // A key introducing a nested list/map: keep its raw following lines.
      const block: string[] = [];
      while (i < lines.length && (/^\s/.test(lines[i]) || lines[i].trim() === "")) {
        block.push(lines[i].trim());
        i++;
      }
      value = block.join(" ").trim();
    } else {
      value = unquote(value);
    }
    out[key] = value;
  }
  return out;
}

export function parseSkillMd(text: string): ParsedSkillMd {
  const split = splitFrontmatter(text);
  if (!split) {
    return { frontmatter: null, frontmatterText: "", body: text, name: null, description: null };
  }
  const frontmatter = parseSimpleYaml(split.yaml);
  const name = frontmatter.name?.trim() || null;
  const description = frontmatter.description?.trim() || null;
  return { frontmatter, frontmatterText: split.yaml, body: split.body, name, description };
}

/**
 * Run the five checks. `existingNames` is the bank's current labels (any case);
 * pass `undefined` to leave the `unique` check pending (e.g. before the bank
 * loaded). `serverDuplicate` forces the `unique` check red after a 409.
 */
export function validateSkillMd(
  text: string,
  existingNames?: readonly string[],
  serverDuplicate = false,
): SkillMdValidation {
  const parsed = parseSkillMd(text);
  const checks: SkillCheck[] = [];
  let reason: string | null = null;

  const hasFrontmatter = parsed.frontmatter !== null;
  checks.push({
    id: "frontmatter",
    state: hasFrontmatter ? "pass" : "fail",
    label: hasFrontmatter ? "Frontmatter block found" : "No frontmatter block",
  });
  if (!hasFrontmatter) {
    reason =
      text.trim() === ""
        ? null
        : "The text has no frontmatter block (`---` … `---`). The harness would ignore this skill, so nothing was written.";
  }

  const name = parsed.name;
  let nameState: CheckState = "fail";
  let nameLabel = "name missing";
  if (!hasFrontmatter) {
    nameState = "pending";
    nameLabel = "name kebab-case";
  } else if (name && isKebabCase(name)) {
    nameState = "pass";
    nameLabel = "name kebab-case";
  } else if (name) {
    nameLabel = "name is not kebab-case";
    reason ??= `\`name: ${name}\` is not kebab-case (lowercase letters, digits, single hyphens). Nothing was written.`;
  } else {
    reason ??= "The frontmatter has no `name`. The harness would ignore this skill, so nothing was written.";
  }
  checks.push({ id: "name", state: nameState, label: nameLabel });

  const description = parsed.description;
  let descState: CheckState = "fail";
  let descLabel = "description missing";
  if (!hasFrontmatter) {
    descState = "pending";
    descLabel = "description present";
  } else if (description) {
    descState = "pass";
    descLabel = "description present";
  } else {
    reason ??= "The frontmatter has no `description`. The harness would ignore this skill, so nothing was written.";
  }
  checks.push({ id: "description", state: descState, label: descLabel });

  const bodyOk = parsed.body.trim().length > 0;
  const bodyState: CheckState = !hasFrontmatter ? "pending" : bodyOk ? "pass" : "fail";
  if (hasFrontmatter && !bodyOk) {
    reason ??= "The body after the frontmatter is empty. Nothing was written.";
  }
  checks.push({
    id: "body",
    state: bodyState,
    label: bodyState === "fail" ? "Body is empty" : "Body is not empty",
  });

  let uniqueState: CheckState = "pending";
  let uniqueLabel = "Name unique in the bank";
  if (serverDuplicate) {
    uniqueState = "fail";
    uniqueLabel = "Name already taken";
  } else if (name && existingNames) {
    const lower = name.toLowerCase();
    const clash = existingNames.find((existing) => existing.toLowerCase() === lower);
    if (clash) {
      uniqueState = "fail";
      uniqueLabel = "Name already taken";
      if (nameState === "pass" && descState === "pass" && bodyOk) {
        reason ??= `\`${clash}\` exists (names are case-insensitive). Rename in the frontmatter, or open the existing skill.`;
      }
    } else {
      uniqueState = "pass";
    }
  }
  if (serverDuplicate && name) {
    reason ??= `\`${name}\` exists (names are case-insensitive). Rename in the frontmatter, or open the existing skill.`;
  }
  checks.push({ id: "unique", state: uniqueState, label: uniqueLabel });

  const valid = checks.every((check) => check.state === "pass");
  return { parsed, checks, valid, reason };
}

/** `1.2 kB`-style size for the Files tab. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} kB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** `updated 2 min ago`-style relative time; `now` is injectable for tests. */
export function timeAgo(iso: string, now: number = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days} day${days === 1 ? "" : "s"} ago`;
  const months = Math.round(days / 30);
  if (months < 12) return `${months} month${months === 1 ? "" : "s"} ago`;
  const years = Math.round(days / 365);
  return `${years} year${years === 1 ? "" : "s"} ago`;
}
