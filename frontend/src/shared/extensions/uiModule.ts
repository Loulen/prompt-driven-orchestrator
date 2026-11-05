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
 * (`SearchResult`, `SearchFunction`); nothing more is exported. Since 0.19 it
 * hands out the core's session terminal (`SessionTerminal`), for any session
 * by its id, and the Zoom du terminal of a Session hébergée
 * (`useTerminalZoom`, `TerminalZoom`); and the bricks of a Session
 * hébergée's detail, which the extension lays out (`SessionHeader`,
 * `SessionActions`, `SessionWaiting`, `SessionTabs`, `LiveArtifacts`,
 * `SessionInfo`, `SessionConfig`, `SessionEvents`), with their live state
 * (`useHostedSession`). Since 0.20 a person is one round avatar in its hue
 * everywhere (`Avatar` follows its Actor, `AvatarStack`, `personColor` for
 * the marks), and `useActorDisplays` names a list of Actors at once. Since
 * 0.22 it opens the core's New Run form prefilled (`openNewRun`, the
 * `NewRunPrefill` of `contract.ts`): the caller names itself in `extension`
 * for its `run/new` sections to receive `data`.
 */

import { useActorDisplay, useActorDisplays, useCurrentActor } from "../actors/store";
import { openProbedSocket } from "../http/probedSocket";
import { copyToClipboard } from "../lib/clipboard";
import SessionTerminal from "../../session/components/SessionTerminal";
import TerminalZoom from "../../session/components/TerminalZoom";
import { useTerminalZoom } from "../../session/store/useTerminalZoom";
import { useHostedSession } from "../../session/store/useHostedSession";
import LiveArtifacts from "../../session/components/hosted/LiveArtifacts";
import SessionActions from "../../session/components/hosted/SessionActions";
import SessionConfig from "../../session/components/hosted/SessionConfig";
import SessionEvents from "../../session/components/hosted/SessionEvents";
import SessionHeader from "../../session/components/hosted/SessionHeader";
import SessionInfo from "../../session/components/hosted/SessionInfo";
import SessionTabs from "../../session/components/hosted/SessionTabs";
import SessionWaiting from "../../session/components/hosted/SessionWaiting";
import * as kit from "../ui/kit";
import { HostedContribution, HostedContributions } from "./HostedContributions";
import { runHostedCommand, useHostedContributions } from "./hosted";
import { openNewRun } from "./newRun";
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
  useActorDisplays,
  openProbedSocket,
  copyToClipboard,
  openNewRun,
  Avatar: kit.Avatar,
  AvatarStack: kit.AvatarStack,
  personColor: kit.personColor,
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
  SessionTerminal,
  useTerminalZoom,
  TerminalZoom,
  useHostedSession,
  SessionHeader,
  SessionActions,
  SessionWaiting,
  SessionTabs,
  LiveArtifacts,
  SessionInfo,
  SessionConfig,
  SessionEvents,
};

export type ExtensionUiModule = typeof extensionUiModule;
