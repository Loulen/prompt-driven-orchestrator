// The routes of the caller's Agent de support: one per Actor, outside any Run.

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { OpenSupportRequest } from "../../shared/generated/OpenSupportRequest";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";
import { request } from "../../shared/http/client";

/** The caller's support agent: whether it lives, its agent, the last choice. */
export function fetchSupportAgent(): Promise<SupportStatusResponse> {
  return request<SupportStatusResponse>("GET", "/sessions/support", { label: "support agent" });
}

/** Open it on `choice`, `question` as its first message; `replace` closes the
 *  one open first. Refused while one lives without `replace`. */
export function openSupportAgent(choice: AgentChoice, question: string, replace: boolean): Promise<SupportStatusResponse> {
  const body: OpenSupportRequest = { choice, replace };
  if (question.trim()) body.question = question.trim();
  return request<SupportStatusResponse>("POST", "/sessions/support", { body, label: "Open support agent" });
}

/** Type `question` into the live agent and submit it. */
export function askSupportAgent(question: string): Promise<SupportStatusResponse> {
  return request<SupportStatusResponse>("POST", "/sessions/support/ask", {
    body: { question },
    label: "Ask the support agent",
  });
}

/** End the agent. */
export function closeSupportAgent(): Promise<SupportStatusResponse> {
  return request<SupportStatusResponse>("DELETE", "/sessions/support", { label: "End session" });
}

/** « Keep »: the agent stays, its idle clock starts over. */
export function keepSupportAgent(): Promise<SupportStatusResponse> {
  return request<SupportStatusResponse>("POST", "/sessions/support/keep", { label: "Keep" });
}
