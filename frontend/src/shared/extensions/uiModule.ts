/**
 * What the shared module `@pdo/extension-ui` exports (ADR-0081, amendment
 * #32): the front puts this object on `globalThis.__PDO_SHARED__`
 * (`shared.ts`), and `public/shared/pdo-extension-ui.js` re-exports it.
 * Since 0.8 it also names the Actors: `useCurrentActor()`, the UI's own, and
 * `useActorDisplay(actor)`, any, as the extensions name them. Since 0.10 it
 * opens the extensions' WebSockets through the probe: `openProbedSocket`
 * (ADR-0083).
 */

import { useActorDisplay, useCurrentActor } from "../actors/store";
import { openProbedSocket } from "../http/probedSocket";
import { HostedContribution, HostedContributions } from "./HostedContributions";
import { runHostedCommand, useHostedContributions } from "./hosted";
import { UI_MODULE_MAJOR, UI_MODULE_VERSION } from "./uiModuleVersion";

export const extensionUiModule = {
  version: UI_MODULE_VERSION,
  major: UI_MODULE_MAJOR,
  useHostedContributions,
  runHostedCommand,
  HostedContribution,
  HostedContributions,
  useCurrentActor,
  useActorDisplay,
  openProbedSocket,
};

export type ExtensionUiModule = typeof extensionUiModule;
