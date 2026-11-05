// The front's shared module `@pdo/extension-ui`, for the extensions' UI bundles
// (ADR-0081, amendment #32): the import map of index.html points the bare
// specifier here, and the front puts the module on `globalThis.__PDO_SHARED__`
// before it renders anything (src/shared/extensions/uiModule.ts). A bundle lists
// and renders the contributions to the locations it hosts through it, draws its
// controls with the primitives of the shared design system (ADR-0091), shows a
// session's terminal with the core's own, composes the detail of a Session
// hébergée from the core's bricks, opens the core's New Run prefilled, and
// never ships its own copy.
const ExtensionUi = globalThis.__PDO_SHARED__.extensionUi;
export default ExtensionUi;
export const {
  version,
  major,
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
  Avatar,
  AvatarStack,
  personColor,
  Banner,
  Button,
  Checkbox,
  Chip,
  ConfirmDialog,
  EmptyState,
  Field,
  IconButton,
  Kbd,
  KeyValue,
  Menu,
  Modal,
  Popover,
  Section,
  Segmented,
  Select,
  StatusIcon,
  Switch,
  Tabs,
  TextArea,
  TextInput,
  toast,
  Tooltip,
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
} = ExtensionUi;
