/**
 * The Commandes d'extension (ADR-0081, amendment #29): an action names one of
 * its extension's commands; the core calls the function the bundle exports for
 * it with the action's location context, and a host to answer back. A throw or
 * a rejection reaches the operator in an error toast; nothing else breaks.
 */

import { toast } from "../ui/toast";
import type { ContributionElement } from "./catalogue";
import type { CommandContext, CommandFunction, CommandHost, LocationContext } from "./contract";
import { followDeepLink } from "./deepLink";
import { openNewRun } from "./newRun";

/** What a command of `extension` may do back. */
export function commandHost(extension: string): CommandHost {
  return {
    notify: (message, kind = "info") => toast(String(message), kind === "error" ? "error" : "info"),
    open: (link) => followDeepLink(link),
    // API 0.22: the data goes to the command's own extension, whatever it says.
    openNewRun: (prefill) => openNewRun({ ...(prefill ?? {}), extension }),
  };
}

/** Call the command of the action `element`, on its location's `context`. */
export async function runCommand(element: ContributionElement, context: LocationContext): Promise<void> {
  const { command, module, extension, contribution } = element;
  if (!command) return;
  const fn = module[command.export];
  if (typeof fn !== "function") {
    toast(`The command “${command.title}” of the extension ${extension.name} is not available.`, "error");
    return;
  }
  const commandContext: CommandContext = {
    ...context,
    extension: { id: extension.id, name: extension.name },
    contribution: contribution.id,
  };
  try {
    await (fn as CommandFunction)(commandContext, commandHost(extension.id));
  } catch (cause) {
    console.warn(`extension ${extension.id}: the command ${command.id} failed`, cause);
    const reason = cause instanceof Error ? cause.message : String(cause);
    toast(`“${command.title}” failed: ${reason}`, "error");
  }
}
