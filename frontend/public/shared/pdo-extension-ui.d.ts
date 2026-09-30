// The types of the shared module `@pdo/extension-ui` (ADR-0081, amendment #32),
// for the authors of extension bundles: copy this file into the bundle's
// sources as `pdo-extension-ui.d.ts` and wrap it in
// `declare module "@pdo/extension-ui" { … }`. The implementation is
// src/shared/extensions/uiModule.ts; docs/reference/extension-ui.md documents it.
import type { ReactNode } from "react";

export interface ExtensionRef {
  id: string;
  name: string;
}

/** What the host hands its elements, on top of every location's context. */
export type HostContext = Record<string, unknown>;

/** The context an element of a hosted location receives: the selected Run,
 *  its Projet, `location`, and the host's keys. */
export type HostedContext = HostContext & {
  location: string;
  run: { id: string; name: string | null; pipeline: string; status: string; attributes: Record<string, string> } | null;
  project: { id: string; name: string; attributes: Record<string, string> } | null;
};

export interface HostedElement {
  /** `<extension id>:<contribution id>`. */
  key: string;
  extension: ExtensionRef;
  id: string;
  label: string;
  type: "page" | "action" | "tab" | "section" | "decoration" | "list_filter";
  context: HostedContext;
}

/** The version of `pdo-extension-api` the module comes with, and its major. */
export const version: string;
export const major: number;

export function useHostedContributions(location: string, context?: HostContext): HostedElement[];
export function runHostedCommand(element: HostedElement): Promise<void>;
export function HostedContribution(props: {
  element: HostedElement;
  props?: Record<string, unknown>;
}): ReactNode;
export function HostedContributions(props: {
  location: string;
  context?: HostContext;
  props?: Record<string, unknown>;
  wrap?: (element: HostedElement, rendered: ReactNode) => ReactNode;
}): ReactNode;

declare const ExtensionUi: {
  version: string;
  major: number;
  useHostedContributions: typeof useHostedContributions;
  runHostedCommand: typeof runHostedCommand;
  HostedContribution: typeof HostedContribution;
  HostedContributions: typeof HostedContributions;
};
export default ExtensionUi;
