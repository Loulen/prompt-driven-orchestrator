// The types of the shared module `@pdo/extension-ui` (ADR-0081, amendment #32),
// for the authors of extension bundles: copy this file into the bundle's
// sources as `pdo-extension-ui.d.ts` and wrap it in
// `declare module "@pdo/extension-ui" { … }`. The implementation is
// src/shared/extensions/uiModule.ts; docs/reference/extension-ui.md documents it.
import type {
  ButtonHTMLAttributes,
  ComponentType,
  ForwardRefExoticComponent,
  InputHTMLAttributes,
  ReactElement,
  ReactNode,
  RefAttributes,
  TextareaHTMLAttributes,
} from "react";

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
  type: "page" | "action" | "panel" | "tab" | "section" | "decoration" | "list_filter" | "completion" | "search";
  context: HostedContext;
}

/** Who triggers a mutation (since 0.8): the core's `Actor`, as on the wire. */
export type Actor =
  | { kind: "local_operator" }
  | { kind: "agent"; run_id: string; node_id: string }
  | { kind: "trigger"; trigger_id: string }
  | { kind: "identified"; namespace: string; id: string };

/** How an Actor shows, as an extension names it (since 0.8). */
export interface ActorDisplay {
  name: string;
  avatar_url: string | null;
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
/** The UI's own Actor and its display name; `null` until known (since 0.8). */
export function useCurrentActor(): { actor: Actor; display: ActorDisplay | null } | null;
/** The display name of `actor`; `null` while unknown or when no extension
 *  names it (since 0.8). */
export function useActorDisplay(actor: Actor | null | undefined): ActorDisplay | null;
/** The display names of `actors`, in their order (`null` while unknown, or when no extension names
 *  them), asked for together: one hook for a list of any length, a stack of avatars say. Since 0.20. */
export function useActorDisplays(actors: readonly Actor[]): (ActorDisplay | null)[];

/** Where a probed socket stands (since 0.10): probing or connecting for the
 *  first time, open, or down (probing again, or closed for good). */
export type ProbedSocketState = "connexion" | "ouvert" | "hors_ligne";

/** A WebSocket to the daemon, opened through the probe (since 0.10). */
export interface ProbedSocket {
  readonly state: ProbedSocketState;
  /** Send when open; whether it was sent. */
  send(data: string | ArrayBuffer | ArrayBufferView<ArrayBuffer> | Blob): boolean;
  /** Every message: text as is, binary as an `ArrayBuffer`. Returns the unsubscribe. */
  onMessage(listener: (data: string | ArrayBuffer) => void): () => void;
  /** Every change of state (`ouvert` at each opening). Returns the unsubscribe. */
  onState(listener: (state: ProbedSocketState) => void): () => void;
  /** Close for good, with 1000. */
  close(): void;
}

/** Open `path` (`/ext/<id>/…`) on the daemon of the page, only after an HTTP
 *  probe succeeded; while it fails, the probe repeats (backoff with jitter), a
 *  hidden tab does not probe, a refused session goes to the sign-in page.
 *  `reconnect: false`: never reopened once it ended; `"after-drop"`: reopened
 *  only when it dropped without the daemon's Close. Since 0.10.
 *  `silenceMs` (since 0.11): an open socket that received nothing for that
 *  long, or whose browser went offline, is closed and counts as a drop; back
 *  online, a pending probe is sent at once. */
export function openProbedSocket(
  path: string,
  options?: { reconnect?: boolean | "after-drop"; silenceMs?: number },
): ProbedSocket;

/** Put `text` on the clipboard from a user gesture: the Clipboard API, or
 *  outside a secure context (an instance served over plain http) a fallback
 *  through `execCommand("copy")`. Resolves to whether the text was written;
 *  never rejects. Since 0.11. */
export function copyToClipboard(text: string): Promise<boolean>;

/** What the core's New Run form opens with (since 0.22): every field optional,
 *  each shown and left editable. A Projet the form does not offer is ignored
 *  (its default applies); `repositories[0]` is the primary repository, the
 *  others its secondaries. The Pipeline stays the operator's choice. */
export interface NewRunPrefill {
  /** The id of the Projet to launch in. */
  project?: string;
  /** The target repositories, the primary first. */
  repositories?: string[];
  /** The Run's name (auto-naming is turned off). */
  name?: string;
  /** The prompt of the entry node. */
  input?: string;
  /** The id of the calling extension: only its `run/new` sections receive `data`
   *  (declared, not checked: its `validate_create` is the boundary). */
  extension?: string;
  /** Handed to the caller's `run/new` sections as `context.prefill`; they
   *  provide it to the creation as usual (`form.provide`). */
  data?: unknown;
}

/** Open the core's New Run form, prefilled with `prefill`; without one, as the
 *  core's New Run button opens it. `false` where no New Run can open (the
 *  Review page), and while one is already open. Since 0.22. */
export function openNewRun(prefill?: NewRunPrefill): boolean;

/** The deep link toward the Run `runId`: `#/runs/<run>`. Since 0.23. */
export function runLink(runId: string): string;

/** The deep link toward the Node `nodeId` of the Run `runId`, at its iteration
 *  `iter` when given: `#/runs/<run>/nodes/<node>[/iter/<n>]`, each segment
 *  percent-encoded, as the daemon writes it (`node_link`). Opened in PDO, it
 *  selects the Run, then the Node at that iteration, and shows its detail with
 *  its terminal; a Node the Run does not hold opens the Run alone. Prefix it
 *  with the instance's address (`location.origin`) to copy it. Since 0.23. */
export function nodeLink(runId: string, nodeId: string, iter?: number | null): string;

// ---------------------------------------------------------------- the command field (since 0.18)
// What a Source de recherche (`type: "search"` at `palette`) exports and
// answers. Types only: the core calls the function, groups the results under
// the extension's name and draws them.

/** The item selected in the interface when the command field opened. */
export interface PaletteSelection {
  /** `run`, `node`, `trigger` or `pipeline`. */
  kind: string;
  id: string;
  name?: string | null;
  attributes: Record<string, string>;
}

/** What a global Action of `palette` and a Source de recherche receive. */
export type PaletteContext = HostedContext & { location: "palette"; selected: PaletteSelection | null };

/** A person on a result (since 0.20): its image, or its initials in its hue. With `actor`, the hue is
 *  that Actor's everywhere in the core. */
export interface RowAvatar {
  name: string;
  actor?: Actor | null;
  avatar_url?: string | null;
}

/** One result, already filtered by what the Actor may read. */
export interface SearchResult {
  /** Unique within the source's answer. */
  id: string;
  title: string;
  /** A reference before the title, in mono (`N-12`). */
  ref?: string | null;
  /** Facts at the right of the row. */
  meta?: string | null;
  /** A person, the row's mark in place of its status or icon. Since 0.20. */
  avatar?: RowAvatar | null;
  /** An SVG the extension serves, the row's icon when it has no status. */
  icon?: string | null;
  /** Drawn as the core draws its own statuses: the tone's icon, the label as tooltip.
   *  With a `shape` (since 0.20), that fixed shape in the tone's colour, never spinning. */
  status?: { tone: StatusTone; label: string; shape?: StatusShape | null } | null;
  /** Where choosing it goes: `#/ext/<extension>/<location>/<path>` or `#/runs/<id>`. */
  link: string;
  /** The ranges `[start, end)` of `title` the query matched. */
  highlights?: [number, number][] | null;
}

/** A Source de recherche: called 120 ms after the last keystroke, never with
 *  an empty query; `signal` aborts when the query changes. Given up on after
 *  5 s; the first 50 results are kept. */
export type SearchFunction = (
  query: string,
  context: PaletteContext,
  signal: AbortSignal,
) => SearchResult[] | Promise<SearchResult[]>;

// ---------------------------------------------------------------- design system (since 0.17)
// The primitives of the shared design system (ADR-0091): the core draws its
// controls with them, in its tokens (`/shared/tokens.css`). Dense: 24 px
// controls and rows, 13 px text, 2 px corners, each Theme and mode, the UI scale.

/** An icon component: a `lucide-react` icon, or anything taking the same props. */
export type Icon = ComponentType<{ size?: number | string; strokeWidth?: number | string; className?: string; "aria-hidden"?: boolean | "true" }>;

/** The six status tones of the palette. */
export type StatusTone = "running" | "awaiting" | "done" | "blocked" | "failed" | "skipped";
/** A status tone, or none. */
export type Tone = "neutral" | StatusTone;
/** The fixed shape of a status that is no Run's (since 0.20): to do, in progress, in review, done. */
export type StatusShape = "todo" | "doing" | "review" | "done";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon?: Icon;
  /** primary: the one main action of a view (ruby). danger: a destructive confirm. ghost: no border until hover. */
  tone?: "default" | "primary" | "ghost" | "danger";
  /** 24 px (`md`) or 20 px (`sm`). */
  size?: "md" | "sm";
}
/** A text button: hairline, no fill. */
export const Button: ForwardRefExoticComponent<ButtonProps & RefAttributes<HTMLButtonElement>>;

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: Icon;
  /** Accessible name and tooltip: the button has no visible text. */
  label: string;
  kbd?: string;
  /** A toggle that is on (`aria-pressed`). */
  pressed?: boolean;
  size?: "md" | "sm";
  /** A small count in the corner. */
  badge?: number | null;
  tone?: "danger";
  /** One short extra line in the tooltip. */
  description?: string;
}
/** An icon-only button, its name and shortcut in a tooltip. */
export const IconButton: ForwardRefExoticComponent<IconButtonProps & RefAttributes<HTMLButtonElement>>;

/** A key or a chord: hairline, no fill, mono. */
export function Kbd(props: { children: ReactNode; title?: string }): ReactNode;

export interface TooltipProps {
  /** The control's name. */
  label: string;
  kbd?: string;
  /** One short line when the name is not enough (a disabled reason, a count). */
  description?: string;
  side?: "bottom" | "top";
  delay?: number;
  disabled?: boolean;
  /** One control; it keeps its own `aria-label`. */
  children: ReactElement;
}
/** Wraps one control: its name and shortcut as text ("Retry (R)"). */
export function Tooltip(props: TooltipProps): ReactNode;

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  mono?: boolean;
  /** The failed edge, and `aria-invalid`. */
  invalid?: boolean;
  /** 20 px instead of 24. */
  small?: boolean;
}
export const TextInput: ForwardRefExoticComponent<TextInputProps & RefAttributes<HTMLInputElement>>;

export interface TextAreaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  mono?: boolean;
  invalid?: boolean;
}
export const TextArea: ForwardRefExoticComponent<TextAreaProps & RefAttributes<HTMLTextAreaElement>>;

export interface FieldProps {
  label: string;
  /** Help goes in the label's tooltip, never under the field. */
  help?: string;
  /** Short, 2 to 5 words. */
  error?: string | null;
  /** The control. It gets `id`, `invalid` and `aria-describedby`. */
  children: ReactElement;
}
/** A form row: the label in a 96 px column, the control, the error at the row end. */
export function Field(props: FieldProps): ReactNode;

export interface SelectOption<V extends string = string> {
  value: V;
  label: string;
  icon?: Icon;
  /** Right-aligned meta in the row. */
  hint?: string;
  /** Rows of the same group are listed under a caps header. */
  group?: string;
  disabled?: boolean;
  title?: string;
}
export interface SelectProps<V extends string = string> {
  value: V | null;
  onChange: (value: V) => void;
  options: SelectOption<V>[];
  /** Accessible name and tooltip. */
  label: string;
  placeholder?: string;
  icon?: Icon;
  mono?: boolean;
  size?: "md" | "sm";
  disabled?: boolean;
  /** A search box over the list; by default when there are more than 8 options. */
  search?: boolean;
  align?: "start" | "end";
  /** A filter in force: the trigger is tinted. Since 0.20. */
  active?: boolean;
  className?: string;
}
/** A single-value dropdown under a bordered 24 px trigger; its tooltip is the value shown. */
export function Select<V extends string>(props: SelectProps<V>): ReactNode;

export interface SegmentedItem<T extends string = string> {
  value: T;
  label?: string;
  icon?: Icon;
  /** Tooltip, and the name of an icon-only item. */
  title?: string;
  disabled?: boolean;
}
export interface SegmentedProps<T extends string = string> {
  items: SegmentedItem<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Accessible name of the group. */
  label: string;
  size?: "md" | "sm";
  disabled?: boolean;
  /** Full width: the items share the line equally, a long label ends in an ellipsis. */
  fill?: boolean;
}
/** Two to four exclusive choices. */
export function Segmented<T extends string>(props: SegmentedProps<T>): ReactNode;

/** A square on/off switch, ruby when on. */
export function Switch(props: { checked: boolean; onChange: (checked: boolean) => void; label: string; disabled?: boolean }): ReactNode;

/** The native box, in the ruby accent; `children` is the visible text after it. */
export function Checkbox(props: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  children?: ReactNode;
  disabled?: boolean;
  /** Some, not all (a select-all box). */
  mixed?: boolean;
}): ReactNode;

export interface PopoverProps {
  trigger: (p: { open: boolean; toggle: () => void }) => ReactNode;
  children: ReactNode | ((close: () => void) => ReactNode);
  /** Accessible name of the panel. */
  label: string;
  align?: "start" | "end";
  className?: string;
}
/** A non-modal panel under its trigger. */
export function Popover(props: PopoverProps): ReactNode;

export interface MenuItem {
  kind?: "item";
  label: string;
  icon?: Icon;
  kbd?: string;
  /** Right-aligned mono meta. */
  hint?: string;
  /** A check item, or a radio item with `radio`. */
  checked?: boolean;
  radio?: boolean;
  danger?: boolean;
  disabled?: boolean;
  onSelect?: () => void;
  submenu?: MenuEntry[];
}
export type MenuEntry = MenuItem | { kind: "separator" } | { kind: "label"; label: string };
/** A trigger and a menu under it: a check column, the icon, the label, the shortcut. */
export function Menu(props: {
  trigger: (p: { open: boolean; toggle: () => void }) => ReactNode;
  entries: MenuEntry[];
  label: string;
  align?: "start" | "end";
}): ReactNode;

export interface ModalProps {
  title: ReactNode;
  /** Plain-text title, for the accessible name when `title` is an element. */
  titleText?: string;
  icon?: ReactNode;
  children?: ReactNode;
  /** Left of the footer: 11 px meta. */
  footerNote?: ReactNode;
  /** Right of the footer: buttons, the primary last. */
  actions?: ReactNode;
  onClose: () => void;
  /** false: no close control, Escape and the backdrop do nothing. */
  dismissable?: boolean;
  /** s 380, m 500, l 680 px wide. */
  size?: "s" | "m" | "l";
  /** Run by Ctrl+Enter. */
  onSubmit?: () => void;
  "data-testid"?: string;
}
/** A native modal dialog: title bar, body, footer. Mount to open, unmount to close. */
export function Modal(props: ModalProps): ReactNode;

export interface ConfirmDialogProps {
  /** The action and its object: "Delete environment staging?". */
  title: string;
  /** At most 5 words, destructive only. */
  consequence?: string;
  /** The object, in mono. */
  target?: string;
  actionLabel: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
  children?: ReactNode;
}
/** A confirmation: Enter or Ctrl+Enter confirms, Escape cancels. */
export function ConfirmDialog(props: ConfirmDialogProps): ReactNode;

export type ToastKind = "info" | "success" | "warning" | "error";
/** Show `text` for a few seconds in the core's toast stack. */
export function toast(
  text: string,
  kind?: ToastKind,
  options?: { title?: string; actionLabel?: string; onAction?: () => void },
): void;

export interface TabItem<T extends string = string> {
  value: T;
  label: string;
  icon?: Icon;
  count?: number;
  kbd?: string;
  disabled?: boolean;
}
/** A tab strip, the live tab underlined in ruby. */
export function Tabs<T extends string>(props: { items: TabItem<T>[]; value: T; onChange: (value: T) => void; label: string }): ReactNode;

/** A section: caps head with a chevron and a count; folds. */
export function Section(props: {
  title: string;
  count?: number;
  /** Icon buttons at the right of the head, shown on hover. */
  aside?: ReactNode;
  /** Starts folded. */
  closed?: boolean;
  /** A body without side padding. */
  flush?: boolean;
  children?: ReactNode;
}): ReactNode;

export interface KeyValueRow {
  k: string;
  v: ReactNode;
  mono?: boolean;
  tip?: string;
}
/** A read-only property grid. */
export function KeyValue(props: { rows: KeyValueRow[] }): ReactNode;

/** A tinted line at the top of a pane: the status icon (or `icon`, in the
 *  tone's colour, since 0.20: a lock on a read-only state), a short fact, the
 *  detail, actions. */
export function Banner(props: {
  tone: StatusTone;
  title: string;
  text?: string;
  actions?: ReactNode;
  icon?: Icon;
}): ReactNode;

/** An icon, 2 to 4 words, one action. */
export function EmptyState(props: {
  icon: Icon;
  children: ReactNode;
  action?: ReactNode;
  layout?: "center" | "row";
  spin?: boolean;
}): ReactNode;

/** A neutral chip; a tone shows as its status icon. */
export function Chip(props: {
  children: ReactNode;
  tone?: Tone;
  icon?: Icon;
  mono?: boolean;
  title?: string;
  onRemove?: () => void;
  removeLabel?: string;
}): ReactNode;

/** A Run's or a node's status as the core draws it, or a tone with its name. */
export function StatusIcon(
  props: { size?: "sm" | "md" | number; title?: string } & (
    | { kind: "run"; status: "running" | "awaiting_user" | "completed" | "failed" | "skipped" | "halted" | "paused" | "archived" | "stalled" }
    | {
        kind: "node";
        status: "pending" | "waiting" | "running" | "awaiting_user" | "completed" | "skipped" | "failed" | "stopped" | "stale" | "interrupted";
      }
    | { kind: "tone"; status: StatusTone; label: string; shape?: StatusShape | null }
  ),
): ReactNode;

/** A person, the same on every surface (since 0.20): round, a tint of its hue, its initials (first and
 *  last word). The hue follows `actor` (its name without one), so the core and every extension draw the
 *  same person alike. An agent or a trigger keeps a neutral fill and its glyph (a robot, a bolt). */
export interface AvatarProps {
  name: string;
  /** Who it is: its hue, and its glyph when it is an agent or a trigger. Since 0.20. */
  actor?: Actor | null;
  /** By default, the Actor's kind. */
  kind?: "human" | "agent" | "trigger";
  /** 16 in rows, 20 in bars and presence, 24 in a profile. */
  size?: 16 | 20 | 24;
  /** An image instead of the initials. */
  src?: string | null;
  /** A deactivated account, or a person who left: dashed, faint. */
  off?: boolean;
  /** Reads only: hollow, a ring in its hue. Since 0.20. */
  hollow?: boolean;
  /** Has it open now: a ring in the done colour. Since 0.20. */
  present?: boolean;
  /** Tooltip and accessible name; the name by default. Since 0.20. */
  title?: string;
  /** `data-testid` and the like, on the avatar. */
  [data: `data-${string}`]: string | undefined;
}
export function Avatar(props: AvatarProps): ReactNode;

/** People overlapping: the first `max` (3), then « +n » whose tooltip lists the others, one per line.
 *  Nothing when `people` is empty. Since 0.20. */
export function AvatarStack(props: {
  people: (AvatarProps & { key?: string })[];
  size?: 16 | 20 | 24;
  max?: number;
  /** Tooltip of the whole stack. */
  title?: string;
}): ReactNode;

/** The colour of a person's marks outside its avatar (a presence outline, a cursor, a name tag): its
 *  avatar's hue, solid, in the current Theme and mode. A CSS colour. Since 0.20. */
export function personColor(person: { name?: string; actor?: Actor | null }): string;

// ---------------------------------------------------------------- the session terminal (since 0.19)

/** The core's session terminal, for any session by its id: its tmux session's
 *  name (a Session hébergée's `id`, `pdo-hosted-…`). Poste, Pilote, Spectateur
 *  and Prise de main, resizing and reconnection are the core's; no xterm to
 *  ship, no protocol to write. Its text follows the session's Zoom du terminal
 *  (live, the same on every Poste) times the `zoom` the host imposes. It fills
 *  its parent, which needs a height. A Session hébergée relaunched after it
 *  ended is connected to anew. Since 0.19. */
export function SessionTerminal(props: {
  session: string;
  /** A zoom the host imposes, as a factor (`1`: none). */
  zoom?: number;
  /** At the head of the toolbar, in place of the connection dot. */
  toolbarIdentity?: ReactNode;
  /** At the right of the toolbar, before Copy. */
  toolbarActions?: ReactNode;
  expanded?: boolean;
  /** Adds the expand button to the toolbar. */
  onExpand?: () => void;
  onLiveSocketChange?: (live: boolean) => void;
  /** Whether this browser only watches (another Poste pilots). */
  onSpectatingChange?: (spectating: boolean) => void;
}): ReactNode;

/** The Zoom du terminal of a session, live: in percent (50 to 200; 100 for a
 *  session that carries none); `hosted` is `null` until the daemon answered.
 *  `setZoom` sets it for every Poste on a Session hébergée (the Operation
 *  `session` × `zoom`) and rejects with the refusal. Since 0.19. */
export function useTerminalZoom(session: string): {
  zoom: number;
  hosted: boolean | null;
  setZoom: (zoom: number) => Promise<void>;
};

/** The control of the Zoom du terminal of a Session hébergée: 50 to 200 %
 *  between − and +, « same for everyone watching ». Off for any other
 *  session. Since 0.19. */
export function TerminalZoom(props: { session: string }): ReactNode;

// ---------------------------------------------------------------- the detail of a Session hébergée (since 0.19)
// The bricks of a session's detail: the extension lays them out (a drawer, a
// pane) and adds its own around them. Each takes the session's id (its `id`,
// `pdo-hosted-…`) and follows it live; they share one subscription per session.

export type HostedKind = "agent" | "shell";
export type HostedState = "starting" | "running" | "awaiting_user" | "ended";

/** How an agent session is launched; a field left out takes the instance's default. */
export interface HostedConfig {
  harness?: string | null;
  model?: string | null;
  effort?: string | null;
  prompt?: string | null;
}

/** A Session hébergée, as the core records it. */
export interface HostedSession {
  id: string;
  extension: string;
  key: string;
  kind: HostedKind;
  config: HostedConfig;
  folder: { kind: "path"; path: string } | { kind: "project_repo" } | { kind: "home" };
  cwd: string;
  project: string | null;
  started_by: Actor | null;
  state: HostedState;
  end_reason: "terminated" | "exited" | null;
  wait_message?: string | null;
  terminal_zoom: number;
  created_at: string;
  updated_at: string;
}

/** One Artefact live: a file its agent wrote in its `PDO_LIVE_DIR`. */
export interface HostedLiveArtifact {
  path: string;
  size: number;
  modified_at: string;
}

/** One entry of its journal: what happened to it, when, by whom. */
export interface HostedSessionEvent {
  at: string;
  kind: "opened" | "running" | "awaiting_user" | "resumed" | "terminated" | "exited" | "relaunched" | "configured";
  by?: Actor;
  message?: string;
}

/** A Session hébergée by its id, live: read from the daemon, then followed on
 *  `/ws`. `session.driving` is who drives its terminal (the Pilote, or the one
 *  Poste); `error.status` is `403` when the Actor may not read it, `404` when
 *  there is no such session (or it was forgotten). Each gesture is the
 *  Opération its extension's would be (`session` × `terminate`, `relaunch`,
 *  `configure`) and rejects with the refusal. Since 0.19. */
export function useHostedSession(session: string): {
  session: (HostedSession & { driving: Actor | null }) | null;
  live: HostedLiveArtifact[];
  events: HostedSessionEvent[];
  loading: boolean;
  error: { status: number | null; message: string } | null;
  terminate: () => Promise<HostedSession>;
  relaunch: () => Promise<HostedSession>;
  configure: (config: HostedConfig) => Promise<HostedSession>;
};

/** Its head: the kind's icon, the name (its key by default), the state, the
 *  harness (the instance's default when none is set); in mono its kind,
 *  how long in that state and the host's `context` (where it sits). `actions` at the right of the first line. Its
 *  folder, Projet, starter and driver are `SessionInfo`'s. */
export function SessionHeader(props: { session: string; name?: string; context?: ReactNode; actions?: ReactNode }): ReactNode;

/** Its gestures: the host's buttons first (`children`: « Full screen »…), then Terminate while
 *  it lives, Relaunch once ended; a refusal shows under them and in a toast. */
export function SessionActions(props: { session: string; children?: ReactNode }): ReactNode;

/** « Waiting for you » and what its agent asked, while it waits; nothing otherwise. */
export function SessionWaiting(props: { session: string }): ReactNode;

/** Its tabs: Details (`details`, what the host composes), Events, then the
 *  tabs of the `session/detail` location, each handed the session. */
export function SessionTabs(props: { session: string; details?: ReactNode }): ReactNode;

/** Its Artefacts live, live: Markdown rendered and open, an image or a text
 *  on its name, each downloadable. */
export function LiveArtifacts(props: { session: string }): ReactNode;

/** What it is: kind, folder, Projet, who started it, who drives it. */
export function SessionInfo(props: { session: string }): ReactNode;

/** The configuration of an agent session (agent, initial prompt), applied at
 *  its next relaunch; a shell says it has none. */
export function SessionConfig(props: { session: string }): ReactNode;

/** Its journal, newest first. */
export function SessionEvents(props: { session: string }): ReactNode;

declare const ExtensionUi: {
  version: string;
  major: number;
  useHostedContributions: typeof useHostedContributions;
  runHostedCommand: typeof runHostedCommand;
  HostedContribution: typeof HostedContribution;
  HostedContributions: typeof HostedContributions;
  useCurrentActor: typeof useCurrentActor;
  useActorDisplay: typeof useActorDisplay;
  useActorDisplays: typeof useActorDisplays;
  openProbedSocket: typeof openProbedSocket;
  copyToClipboard: typeof copyToClipboard;
  openNewRun: typeof openNewRun;
  runLink: typeof runLink;
  nodeLink: typeof nodeLink;
  Avatar: typeof Avatar;
  AvatarStack: typeof AvatarStack;
  personColor: typeof personColor;
  Banner: typeof Banner;
  Button: typeof Button;
  Checkbox: typeof Checkbox;
  Chip: typeof Chip;
  ConfirmDialog: typeof ConfirmDialog;
  EmptyState: typeof EmptyState;
  Field: typeof Field;
  IconButton: typeof IconButton;
  Kbd: typeof Kbd;
  KeyValue: typeof KeyValue;
  Menu: typeof Menu;
  Modal: typeof Modal;
  Popover: typeof Popover;
  Section: typeof Section;
  Segmented: typeof Segmented;
  Select: typeof Select;
  StatusIcon: typeof StatusIcon;
  Switch: typeof Switch;
  Tabs: typeof Tabs;
  TextArea: typeof TextArea;
  TextInput: typeof TextInput;
  toast: typeof toast;
  Tooltip: typeof Tooltip;
  SessionTerminal: typeof SessionTerminal;
  useTerminalZoom: typeof useTerminalZoom;
  TerminalZoom: typeof TerminalZoom;
  useHostedSession: typeof useHostedSession;
  SessionHeader: typeof SessionHeader;
  SessionActions: typeof SessionActions;
  SessionWaiting: typeof SessionWaiting;
  SessionTabs: typeof SessionTabs;
  LiveArtifacts: typeof LiveArtifacts;
  SessionInfo: typeof SessionInfo;
  SessionConfig: typeof SessionConfig;
  SessionEvents: typeof SessionEvents;
};
export default ExtensionUi;
