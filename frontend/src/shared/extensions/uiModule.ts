/**
 * What the shared module `@pdo/extension-ui` exports (ADR-0081, amendment
 * #32): the front puts this object on `globalThis.__PDO_SHARED__`
 * (`shared.ts`), and `public/shared/pdo-extension-ui.js` re-exports it.
 * Since 0.8 it also names the Actors: `useCurrentActor()`, the UI's own, and
 * `useActorDisplay(actor)`, any, as the extensions name them. Since 0.10 it
 * opens the extensions' WebSockets through the probe: `openProbedSocket`
 * (ADR-0083). Since 0.11 it writes to the clipboard the way the core's Copy
 * buttons do, outside a secure context too: `copyToClipboard`. Since 0.17 it
 * hands out the shared design system's primitives (ADR-0091): an extension's
 * UI draws its controls with the core's own, in the core's tokens. Since
 * 0.18 its types describe the Sources de recherche of the command field
 * (`SearchResult`, `SearchFunction`); nothing more is exported.
 */

import { useActorDisplay, useCurrentActor } from "../actors/store";
import { openProbedSocket } from "../http/probedSocket";
import { copyToClipboard } from "../lib/clipboard";
import * as kit from "../ui/kit";
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
  copyToClipboard,
  Avatar: kit.Avatar,
  Banner: kit.Banner,
  Button: kit.Button,
  Checkbox: kit.Checkbox,
  Chip: kit.Chip,
  ConfirmDialog: kit.ConfirmDialog,
  EmptyState: kit.EmptyState,
  Field: kit.Field,
  IconButton: kit.IconButton,
  Kbd: kit.Kbd,
  KeyValue: kit.KeyValue,
  Menu: kit.Menu,
  Modal: kit.Modal,
  Popover: kit.Popover,
  Section: kit.Section,
  Segmented: kit.Segmented,
  Select: kit.Select,
  StatusIcon: kit.StatusIcon,
  Switch: kit.Switch,
  Tabs: kit.Tabs,
  TextArea: kit.TextArea,
  TextInput: kit.TextInput,
  toast: kit.toast,
  Tooltip: kit.Tooltip,
};

export type ExtensionUiModule = typeof extensionUiModule;
