/**
 * The Destinations de bug report « Send to » lists (CONTEXT.md), pure: the
 * core's first, then each extension's under its name, as the daemon offers
 * them; where the window starts (the last choice, else the core's); and the
 * report as `POST /bug-report` carries it.
 */

import type { BugReportDestinations } from "../../shared/generated/BugReportDestinations";
import type { BugReportFields as WireFields } from "../../shared/generated/BugReportFields";
import type { SendBugReport } from "../../shared/generated/SendBugReport";
import { GITHUB_DESTINATION, SECTIONS, type BugReportDestination, type BugReportFields } from "./bugReport";

/** The meta of the row the caller chose last. */
export const LAST_USED = "last used";

/** « Send to »: the core's destination, then the extensions', the last choice marked. */
export function destinationsOf(answer: BugReportDestinations | null): BugReportDestination[] {
  const offered = (answer?.destinations ?? []).map(
    (d): BugReportDestination => ({
      id: d.id,
      label: d.label,
      acceptsPrompts: d.accepts_prompts,
      group: d.extension_name,
      hint: answer?.last === d.id ? LAST_USED : undefined,
      sendLabel: `Send to ${d.label}`,
      icon: d.icon,
      extension: d.extension,
    }),
  );
  return [GITHUB_DESTINATION, ...offered];
}

/** Where the window starts: the last choice when it is still listed, else the core's. */
export function initialDestination(destinations: BugReportDestination[], last: string | null | undefined): string {
  return destinations.some((d) => d.id === last) ? (last as string) : GITHUB_DESTINATION.id;
}

/** Whether the report leaves through the browser (the core's) or through the daemon (an extension's). */
export const opensInBrowser = (destination: BugReportDestination) => destination.id === GITHUB_DESTINATION.id;

/** The fields as the daemon reads them: every section, `null` when absent. */
export function wireFields(fields: BugReportFields): WireFields {
  const out = {} as WireFields;
  for (const { key } of SECTIONS) out[key] = fields[key] ?? null;
  return out;
}

/** The body of `POST /bug-report`: the text the Preview shows, its fields, the screenshot in base64. */
export function reportBody(
  destination: BugReportDestination,
  text: string,
  fields: BugReportFields,
  screenshot: { mediaType: string; base64: string } | null,
): SendBugReport {
  const body: SendBugReport = { destination: destination.id, text, fields: wireFields(fields) };
  if (screenshot) body.screenshot = { media_type: screenshot.mediaType, data: screenshot.base64 };
  return body;
}
