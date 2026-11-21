/**
 * The HTTP client of the Destinations de bug report (CONTEXT.md), over the
 * shared {@link request}: what the extensions offer and the last choice
 * (`GET /bug-report/destinations`), a report sent to one of them
 * (`POST /bug-report`), the core's remembered once the browser opened it
 * (`PUT /bug-report/last`).
 */

import type { BugReportDestinations } from "../../shared/generated/BugReportDestinations";
import type { BugReportSent } from "../../shared/generated/BugReportSent";
import type { SendBugReport } from "../../shared/generated/SendBugReport";
import { request } from "../../shared/http/client";

export function fetchBugReportDestinations(): Promise<BugReportDestinations> {
  return request<BugReportDestinations>("GET", "/bug-report/destinations");
}

/** Sends a report to an extension's destination; rejects with the daemon's sentence. */
export function sendBugReport(body: SendBugReport): Promise<BugReportSent> {
  return request<BugReportSent>("POST", "/bug-report", { body, label: "Send the bug report" });
}

/** Remembers `destination` as the caller's last choice. */
export function rememberBugReportDestination(destination: string): Promise<void> {
  return request<void>("PUT", "/bug-report/last", {
    body: { destination },
    responseMode: "void",
    label: "Remember the bug report destination",
  });
}
