/**
 * The Rapport de bug (CONTEXT.md), pure: what the window's form gives
 * (a description, a Run, a Pipeline, the prompts, a Destination de bug report)
 * plus the instance's facts, turned into its fields and the markdown the
 * Preview tab shows. An edited Preview is read back into fields, so what
 * leaves is the text the user saw. The window decides nothing of this.
 */

import { CORE_NAME } from "../../shared/ui/brand/brand";
import { pipelineDetails, promptDetails, runDetails, type PipelineFacts, type RunFacts } from "./bugReportDetails";

/**
 * Where a Rapport de bug goes (CONTEXT.md « Destination de bug report »): the
 * core's public issues, first and always there; an Extension may declare
 * others. `acceptsPrompts`: whether the prompts may leave with the report.
 */
export interface BugReportDestination {
  id: string;
  label: string;
  acceptsPrompts: boolean;
  /** The header its row sits under in « Send to ». */
  group: string;
  /** The meta of its row: `public` for a public tracker. */
  hint?: string;
  /** The primary button's label: where the report goes. */
  sendLabel: string;
  /** An extension's: the SVG of its row, which the extension serves (`null`: the label's first letter). */
  icon?: string | null;
  /** An extension's: its id. The core's has none. */
  extension?: string;
}

/** The core's destination: a new issue of its public repository, opened prefilled in the browser. */
export const GITHUB_DESTINATION: BugReportDestination = {
  id: "github",
  label: `${CORE_NAME} issues (GitHub)`,
  acceptsPrompts: false,
  group: "Core",
  hint: "public",
  sendLabel: "Open GitHub issue",
};

/** The facts every report carries, whatever the form says. */
export interface InstanceFacts {
  /** The core's `PDO 2.81.0`, or a distribution's `acme-pdo 0.3.0 (core PDO 2.81.0)`. */
  version: string | null;
  /** The daemon's system: `Linux 6.8.0-107-generic`. */
  os: string | null;
  harnesses: { name: string; version: string | null }[];
}

/** The sections of a report, in their order, under the headings of the issue template. */
export const SECTIONS = [
  { key: "description", heading: "Description" },
  { key: "version", heading: "Version" },
  { key: "os", heading: "OS" },
  { key: "harnesses", heading: "Harnesses" },
  { key: "run", heading: "Run details" },
  { key: "pipeline", heading: "Pipeline details" },
  { key: "prompts", heading: "Prompts" },
] as const;

export type SectionKey = (typeof SECTIONS)[number]["key"];

/** A report's fields: its sections' text; a section left out is absent. */
export type BugReportFields = Partial<Record<SectionKey, string>>;

export interface BugReportInput {
  description: string;
  /** `null` until they are read: the report says so. */
  facts: InstanceFacts | null;
  /** The Run joined, when « Include run details » is checked and it is read. */
  run: RunFacts | null;
  /** The Pipeline joined, when « Include pipeline details » is checked and it is read. */
  pipeline: PipelineFacts | null;
  includePrompts: boolean;
  destination: BugReportDestination;
  /** Now, in ms: the duration of what still runs. */
  now: number;
}

const UNKNOWN = "unknown";

/** The fields of a report: the facts always, the details asked for, the prompts only where they may go. */
export function reportFields(input: BugReportInput): BugReportFields {
  const { facts } = input;
  const fields: BugReportFields = {
    description: input.description.trim(),
    version: facts?.version ?? UNKNOWN,
    os: facts?.os ?? UNKNOWN,
    harnesses:
      facts && facts.harnesses.length > 0
        ? facts.harnesses.map((h) => [h.name, h.version].filter(Boolean).join(" ")).join(" · ")
        : UNKNOWN,
  };
  if (input.run) fields.run = runDetails(input.run, input.now);
  if (input.pipeline) {
    fields.pipeline = pipelineDetails(input.pipeline);
    if (input.includePrompts && input.destination.acceptsPrompts) {
      const prompts = promptDetails(input.pipeline);
      if (prompts) fields.prompts = prompts;
    }
  }
  return fields;
}

/** The report as the Preview tab shows it: one `### Heading` per section present. */
export function reportMarkdown(fields: BugReportFields): string {
  return SECTIONS.filter((s) => fields[s.key] !== undefined)
    .map((s) => `### ${s.heading}\n${fields[s.key]}`)
    .join("\n\n");
}

const BY_HEADING = new Map<string, SectionKey>(SECTIONS.map((s) => [s.heading.toLowerCase(), s.key]));

/**
 * The fields of a Preview the user edited: each known `### Heading` opens its
 * section; any other line stays in the section it is in, and what comes
 * before the first heading is description.
 */
export function parseReport(text: string): BugReportFields {
  const fields: BugReportFields = {};
  let key: SectionKey = "description";
  let lines: string[] = [];
  const flush = () => {
    const body = lines.join("\n").trim();
    if (body || fields[key] === undefined) fields[key] = [fields[key], body].filter(Boolean).join("\n\n");
    lines = [];
  };
  for (const line of text.split("\n")) {
    const heading = /^###\s+(.+?)\s*$/.exec(line);
    const next = heading ? BY_HEADING.get(heading[1].toLowerCase()) : undefined;
    if (next) {
      flush();
      key = next;
    } else {
      lines.push(line);
    }
  }
  flush();
  if (fields.description === "") delete fields.description;
  return fields;
}

/** What the form holds that decides whether the report may leave. */
export interface BugReportForm {
  description: string;
  includeRun: boolean;
  runId: string | null;
  includePipeline: boolean;
  pipelineId: string | null;
}

export const RUN_MISSING = "Pick a run, or uncheck Include run details";
export const PIPELINE_MISSING = "Pick a pipeline, or uncheck Include pipeline details";
export const DESCRIPTION_MISSING = "Describe the bug first";
export const RUN_UNREADABLE = "This run could not be read: pick another, or uncheck Include run details";
export const PIPELINE_UNREADABLE = "This pipeline could not be read: pick another, or uncheck Include pipeline details";

/** What stops the sending, field by field: a checked box needs its choice, the description its text. */
export function blockers(form: BugReportForm): { description?: string; run?: string; pipeline?: string } {
  const out: { description?: string; run?: string; pipeline?: string } = {};
  if (form.description.trim() === "") out.description = DESCRIPTION_MISSING;
  if (form.includeRun && !form.runId) out.run = RUN_MISSING;
  if (form.includePipeline && !form.pipelineId) out.pipeline = PIPELINE_MISSING;
  return out;
}

/** Why the report may not leave yet (the primary button's tooltip), else `undefined`. */
export function sendRefusal(form: BugReportForm): string | undefined {
  const b = blockers(form);
  return b.run ?? b.pipeline ?? b.description;
}
