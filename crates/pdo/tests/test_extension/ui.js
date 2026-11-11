// The UI bundle of the internal test extension: one component per location it
// contributes to, and the function of its command.
// A plain ES module, no build step: `react` is the front's own, shared through
// its import map.
import React from "react";
import * as Y from "yjs";
import {
  Avatar,
  AvatarStack,
  Banner,
  Button,
  Chip,
  copyToClipboard,
  EmptyState,
  IconButton,
  LiveArtifacts,
  openNewRun,
  openProbedSocket,
  Segmented,
  SessionActions,
  SessionConfig,
  SessionHeader,
  SessionInfo,
  SessionTabs,
  SessionTerminal,
  SessionWaiting,
  TerminalZoom,
  toast,
  useActorDisplays,
  useCurrentActor,
} from "@pdo/extension-ui";

const h = React.createElement;

// Line icons in the current colour, the shape of the design system's.
function icon(...shapes) {
  return function Icon({ size = 14, strokeWidth = 1.5, className }) {
    return h(
      "svg",
      {
        width: size,
        height: size,
        viewBox: "0 0 24 24",
        fill: "none",
        stroke: "currentColor",
        strokeWidth,
        strokeLinecap: "round",
        strokeLinejoin: "round",
        className,
        "aria-hidden": true,
      },
      ...shapes.map((d, i) => h("path", { key: i, d })),
    );
  };
}
const Lock = icon("M6 11h12v9H6z", "M8 11V7a4 4 0 0 1 8 0v4");
const Star = icon("M12 3l2.8 5.7 6.2.9-4.5 4.4 1 6.2-5.5-2.9-5.5 2.9 1-6.2L3 9.6l6.2-.9z");
const Trash = icon("M4 7h16", "M10 11v6", "M14 11v6", "M6 7l1 13h10l1-13", "M9 7V4h6v3");
const Check = icon("M5 12l5 5 9-10");
const Alert = icon("M12 3l10 18H2z", "M12 10v4", "M12 17h.01");

const box = { display: "flex", flexDirection: "column", gap: "6px", fontSize: "13px" };
const muted = { opacity: 0.7 };

function useJson(url) {
  const [data, setData] = React.useState(null);
  React.useEffect(() => {
    let alive = true;
    fetch(url)
      .then((r) => r.json())
      .then((json) => alive && setData(json))
      .catch(() => alive && setData({ error: true }));
    return () => {
      alive = false;
    };
  }, [url]);
  return data;
}

// What an event is called on the page: `OperationApplied` for the core's
// record of a mutation, the Run event's kind, or the announcement's type.
function eventName(event) {
  if (event.type === "operation_applied") {
    const op = event.operation;
    const verb = typeof op.verb === "string" ? op.verb : op.verb.action;
    return `OperationApplied: ${verb} ${op.resource}${op.target ? " " + op.target : ""}`;
  }
  const kind = event.kind ?? event.event?.kind ?? event.type ?? "event";
  const run = event.run_id ?? event.event?.run_id;
  return run ? `${kind} (${run})` : String(kind);
}

// Navigation entry: the trace of the events the extension observed.
export function EventsPage() {
  const events = useJson("/ext/test/events");
  if (events === null) return h("p", { style: muted }, "Loading…");
  if (!Array.isArray(events)) return h("p", null, "The trace could not be read.");
  return h(
    "div",
    { style: box, "data-testid": "test-extension-events" },
    h("h2", { style: { fontSize: "15px", fontWeight: 600 } }, "Events seen by the test extension"),
    events.length === 0
      ? h("p", { style: muted }, "No event yet.")
      : h(
          "ol",
          { style: { ...box, paddingLeft: "18px", listStyle: "decimal" } },
          events.map((event, i) => h("li", { key: i }, eventName(event))),
        ),
  );
}

// Settings panel: where its data lives, and the switch of the incompatible bundle.
export function SettingsPanel() {
  const about = useJson("/ext/test");
  const [switched, setSwitched] = React.useState(false);
  const activate = () =>
    fetch("/ext/test/incompatible", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: true }),
    }).then(() => setSwitched(true));
  return h(
    "div",
    { style: box, "data-testid": "test-extension-settings" },
    h("p", null, "The internal test extension exercises every hook of the extension API."),
    h("p", { style: muted }, `Data folder: ${about?.data_dir ?? "…"}`),
    h(
      "button",
      { type: "button", onClick: activate, style: { alignSelf: "flex-start", textDecoration: "underline" } },
      switched ? "Incompatible bundle active: reload the page" : "Activate the incompatible bundle",
    ),
  );
}

// List filter (Runs, Triggers, Pipelines, Projects): keep what a given caller
// started. Captioned by its declared label (a 0.1 slot passes none).
export function StartedByFilter({ filter, setFilter, label }) {
  const caption = label ?? "Started by";
  const value = filter["test/started_by"] ?? "";
  return h(
    "label",
    { style: { display: "inline-flex", alignItems: "center", gap: "4px", fontSize: "12px" } },
    caption,
    h("input", {
      "aria-label": caption,
      value,
      placeholder: "anyone",
      style: { width: "90px", background: "transparent", border: "1px solid currentColor", borderRadius: "4px", padding: "0 4px" },
      onChange: (e) => {
        const next = { ...filter };
        if (e.target.value) next["test/started_by"] = e.target.value;
        else delete next["test/started_by"];
        setFilter(next);
      },
    }),
  );
}

// Detail tab of a Run: its attributes.
export function RunTab({ entity }) {
  const attributes = Object.entries(entity.attributes ?? {});
  return h(
    "div",
    { style: box, "data-testid": "test-extension-run-tab" },
    h("p", null, `Run ${entity.id}`),
    attributes.length === 0
      ? h("p", { style: muted }, "No attribute.")
      : h(
          "ul",
          null,
          attributes.map(([key, value]) => h("li", { key }, `${key} = ${value}`)),
        ),
  );
}

// The state of a WebSocket the page opens on the core's `/ws` through the
// probe of `@pdo/extension-ui` (0.10), and how many messages it received.
function useProbedStream() {
  const [state, setState] = React.useState("connexion");
  const [received, setReceived] = React.useState(0);
  React.useEffect(() => {
    const socket = openProbedSocket("/ws");
    socket.onState(setState);
    socket.onMessage(() => setReceived((n) => n + 1));
    return () => socket.close();
  }, []);
  return { state, received };
}

// Page of `topbar/end`: the context it is opened with (the selected Run and its
// Projet), the path of the deep link, in the shell's breadcrumb too, and the
// state of its probed socket. At the path `hosted` (`agent`: `hosted/agent`),
// the demonstration of a Session hébergée (API 0.19).
export function TestPage({ context, navigate, shell }) {
  const path = context?.path ?? "";
  React.useEffect(() => {
    shell?.setBreadcrumb(path ? path.split("/") : []);
  }, [shell, path]);
  // 0.20: each of the two screens names the header its own way.
  const screen = SCREENS[path];
  React.useEffect(() => {
    if (screen) shell?.setTitle?.(screen.title, screen.icon);
  }, [shell, screen]);
  if (path === "hosted" || path.startsWith("hosted/")) {
    return h(HostedSessionDemo, { agent: path === "hosted/agent" });
  }
  if (screen) return h(TestScreen, { path, navigate });
  if (path === "new-run") return h(NewRunDemo);
  return h(TestPageBody, { context, navigate, path });
}

// Two routes of the test page, each a screen of its own (API 0.20): its title
// and its icon in the shell's header, a component for one, an SVG for the other.
const SCREENS = {
  "screens/alpha": { title: "Alpha screen", icon: Star },
  "screens/beta": { title: "Beta screen", icon: "/ext/test/messages.svg" },
};

function TestScreen({ path, navigate }) {
  const other = path === "screens/alpha" ? "screens/beta" : "screens/alpha";
  return h(
    "div",
    { style: { display: "flex", flexDirection: "column", flex: "1 1 auto", minWidth: 0 }, "data-testid": "test-screen" },
    // The read-only state: a lock, in every Theme.
    path === "screens/beta" &&
      h(Banner, { tone: "skipped", icon: Lock, title: "Read-only", text: "Only the owner of this screen edits it" }),
    h(
      "div",
      { style: { ...box, padding: "var(--g-5) var(--g-6)" } },
      h("p", { "data-testid": "test-screen-path" }, `Screen: ${path}`),
      h("span", { style: { alignSelf: "flex-start" } }, h(Button, { size: "sm", onClick: () => navigate(other) }, `Go to ${SCREENS[other].title}`)),
    ),
  );
}

// A text kept in a yjs document (API 0.20): `yjs` is the front's own copy,
// through its import map, so loading this bundle loads no second one.
function useYjsText() {
  return React.useMemo(() => {
    const doc = new Y.Doc();
    doc.getText("note").insert(0, "shared");
    return doc.getText("note").toString();
  }, []);
}

function TestPageBody({ context, navigate, path }) {
  const run = context?.run ?? null;
  const project = context?.project ?? null;
  const stream = useProbedStream();
  const yjs = useYjsText();
  return h(
    "div",
    { style: { ...box, padding: "20px 28px" }, "data-testid": "test-extension-page" },
    h("p", { "data-testid": "test-page-yjs" }, `Yjs: ${yjs}`),
    h("p", { "data-testid": "test-page-run" }, run ? `Run ${run.id} (${run.status})` : "No Run selected"),
    h("p", { "data-testid": "test-page-project" }, project ? `Project ${project.name}` : "No Project"),
    h("p", { "data-testid": "test-page-path" }, `Path: ${path || "(none)"}`),
    h(
      "p",
      { "data-testid": "test-page-socket" },
      `Socket: ${stream.state}${stream.received > 0 ? " (receiving)" : ""}`,
    ),
    h(
      "button",
      { type: "button", onClick: () => navigate("deep/inside"), style: { alignSelf: "flex-start", textDecoration: "underline" } },
      "Go to deep/inside",
    ),
    h(
      "button",
      { type: "button", onClick: () => navigate("hosted"), style: { alignSelf: "flex-start", textDecoration: "underline" } },
      "Hosted session demo",
    ),
    h(
      "button",
      { type: "button", onClick: () => navigate("screens/alpha"), style: { alignSelf: "flex-start", textDecoration: "underline" } },
      "Screens",
    ),
    h(
      "button",
      { type: "button", onClick: () => navigate("new-run"), style: { alignSelf: "flex-start", textDecoration: "underline" } },
      "New Run demo",
    ),
  );
}

// The path `new-run` of the test page (API 0.22): opens the core's New Run
// through `openNewRun` of `@pdo/extension-ui`, prefilled with what the fields
// hold (an empty field prefills nothing), the `cle` field as the data of the
// extension's own `run/new` section; or without any prefill.
function NewRunDemo() {
  const [fields, setFields] = React.useState({ project: "", repositories: "", name: "", input: "", cle: "" });
  const [answer, setAnswer] = React.useState(null);
  const field = (key, label) =>
    h(
      "label",
      { key, style: { display: "flex", gap: "6px", alignItems: "center" } },
      h("span", { style: { width: "110px" } }, label),
      h("input", {
        "data-testid": `test-new-run-${key}`,
        value: fields[key],
        onChange: (e) => setFields({ ...fields, [key]: e.target.value }),
        style: { flex: 1, fontSize: "12px" },
      }),
    );
  const prefilled = () => {
    const repositories = fields.repositories.split(",").map((r) => r.trim()).filter(Boolean);
    return {
      ...(fields.project ? { project: fields.project } : {}),
      ...(repositories.length > 0 ? { repositories } : {}),
      ...(fields.name ? { name: fields.name } : {}),
      ...(fields.input ? { input: fields.input } : {}),
      ...(fields.cle ? { extension: "test", data: { cle: fields.cle } } : {}),
    };
  };
  const open = (prefill) => setAnswer(openNewRun(prefill) ? "opened" : "no New Run here");
  return h(
    "div",
    { style: { ...box, padding: "20px 28px", maxWidth: "560px" }, "data-testid": "test-new-run-demo" },
    field("project", "Project id"),
    field("repositories", "Repositories"),
    field("name", "Name"),
    field("input", "Prompt"),
    field("cle", "Section data (cle)"),
    h(
      "div",
      { style: { display: "flex", gap: "8px" } },
      h(Button, { size: "sm", tone: "primary", onClick: () => open(prefilled()), "data-testid": "test-open-new-run" }, "Open New Run prefilled"),
      h(Button, { size: "sm", onClick: () => open(), "data-testid": "test-open-new-run-plain" }, "Open New Run"),
    ),
    answer && h("p", { style: muted, "data-testid": "test-new-run-answer" }, answer),
  );
}

// The panel of the test messages' icon (`topbar/end`, API 0.20): its items, each
// deleted at once with an « Undo » in the core's toast, which brings it back
// while the panel stays open. Empty: « All caught up ».
export function MessagesPanel({ close }) {
  const [items, setItems] = React.useState(null);
  const [error, setError] = React.useState(null);
  const [attempt, setAttempt] = React.useState(0);
  React.useEffect(() => {
    let alive = true;
    callDemo("GET", "/ext/test/messages").then(
      (list) => alive && (setItems(list), setError(null)),
      (e) => alive && setError(e.message),
    );
    return () => {
      alive = false;
    };
  }, [attempt]);
  const remove = (item) =>
    callDemo("DELETE", `/ext/test/messages/${item.id}`).then(
      (list) => {
        setItems(list);
        toast("Item deleted", "info", {
          actionLabel: "Undo",
          onAction: () =>
            callDemo("POST", `/ext/test/messages/${item.id}/restore`).then(setItems, (e) => toast(e.message, "error")),
        });
      },
      (e) => setError(e.message),
    );
  const head = h(
    "div",
    { className: "d-bar", style: { gap: "var(--g-3)" } },
    h("strong", null, "Messages"),
    items && items.length > 0 && h(Chip, { mono: true, title: `${items.length} items` }, String(items.length)),
  );
  let body;
  if (error && !items) {
    body = h(EmptyState, {
      icon: Alert,
      action: h(Button, { size: "sm", onClick: () => setAttempt((n) => n + 1) }, "Retry"),
    }, "Messages unavailable");
  } else if (!items) {
    body = h(EmptyState, { icon: Check, spin: true }, "Loading");
  } else if (items.length === 0) {
    body = h(EmptyState, { icon: Check }, "All caught up");
  } else {
    body = h(
      "ul",
      { style: { listStyle: "none", margin: 0, padding: "2px", minWidth: 0 }, "data-testid": "test-messages-items" },
      items.map((item) =>
        h(
          "li",
          {
            key: item.id,
            "data-testid": `test-messages-${item.id}`,
            style: { display: "flex", alignItems: "center", gap: "var(--g-3)", minWidth: 0, padding: "2px var(--g-2) 2px var(--pad-x)" },
          },
          // Following an item closes the panel, as a link out of it does.
          h(
            "button",
            {
              type: "button",
              title: item.title,
              onClick: () => {
                close();
                window.location.hash = "#/ext/test/topbar/end/screens/alpha";
              },
              style: {
                flex: "1 1 auto",
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                textAlign: "left",
                background: "none",
                border: 0,
                padding: 0,
                color: "inherit",
                font: "inherit",
                cursor: "pointer",
              },
            },
            item.title,
          ),
          h(IconButton, { icon: Trash, label: `Delete ${item.title}`, size: "sm", tone: "danger", onClick: () => remove(item) }),
        ),
      ),
    );
  }
  return h("div", { style: { display: "flex", flexDirection: "column", minHeight: 0 }, "data-testid": "test-messages" }, head, body);
}

// The demonstration of a Session hébergée (API 0.19): one shell (`demo`), or
// one agent (`demo-agent`), the same for every browser that opens the page.
// Opened on the first visit, through the test extension's Host; its terminal is
// the core's (`SessionTerminal`), and its detail, in a 400 px drawer, is
// composed of the core's bricks: the wait, the header, the gestures (after the
// page's own « Full screen »), the tabs (Details, Events, then the
// `session/detail` tabs of the extensions), and under Details its Artefacts
// live, its Zoom du terminal, what it is and its configuration. A zoom the
// page imposes on top shows how the two zooms combine.
async function callDemo(method, url, body) {
  const response = await fetch(url, {
    method,
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(json.error ?? `${method} ${url}: ${response.status}`);
    error.status = response.status;
    throw error;
  }
  return json;
}

// The session `key`, opened on the first visit; its id once known.
function useDemoSession(key, kind) {
  const [id, setId] = React.useState(null);
  const [error, setError] = React.useState(null);
  const url = `/ext/test/sessions/${key}`;
  React.useEffect(() => {
    let alive = true;
    // Another browser may open it at the same moment: its key is then taken.
    callDemo("GET", url)
      .catch((e) =>
        e.status === 404
          ? callDemo("POST", "/ext/test/sessions", { key, kind }).catch((again) =>
              again.status === 409 ? callDemo("GET", url) : Promise.reject(again),
            )
          : Promise.reject(e),
      )
      .then(
        (session) => alive && setId(session.id),
        (e) => alive && setError(e.message),
      );
    return () => {
      alive = false;
    };
  }, [key, kind, url]);
  return { id, error };
}

const HOST_ZOOMS = [
  { value: "1", label: "None" },
  { value: "0.75", label: "×0.75" },
];

const pad = { padding: "var(--g-4) var(--g-5)" };

function HostedSessionDemo({ agent }) {
  const { id, error } = useDemoSession(agent ? "demo-agent" : "demo", agent ? "agent" : "shell");
  const [hostZoom, setHostZoom] = React.useState("1");
  const [full, setFull] = React.useState(false);
  return h(
    "div",
    { "data-testid": "hosted-demo", style: { display: "flex", flex: 1, width: "100%", height: "100%", minHeight: 0 } },
    h(
      "div",
      { style: { display: "flex", flex: 1, flexDirection: "column", minWidth: 0, minHeight: 0 } },
      id
        ? h(SessionTerminal, {
            session: id,
            zoom: Number(hostZoom),
            toolbarActions: full
              ? h(Button, { size: "sm", onClick: () => setFull(false), "data-testid": "hosted-demo-detail" }, "Show detail")
              : null,
          })
        : h("p", { style: pad, "data-testid": "hosted-demo-loading" }, error ?? "Opening the session…"),
    ),
    id &&
      !full &&
      h(
        "aside",
        {
          "data-testid": "hosted-demo-drawer",
          style: {
            display: "flex",
            flexDirection: "column",
            width: "400px",
            flex: "none",
            minHeight: 0,
            borderLeft: "1px solid var(--line)",
            background: "var(--surface-1)",
            overflow: "auto",
          },
        },
        h(SessionWaiting, { session: id }),
        h(SessionHeader, { session: id, name: agent ? "Demo agent" : "Demo shell", context: "on the test page" }),
        h(
          SessionActions,
          { session: id },
          h(Button, { onClick: () => setFull(true), "data-testid": "hosted-demo-full-screen" }, "Full screen"),
        ),
        h(SessionTabs, {
          session: id,
          details: [
            h(LiveArtifacts, { key: "live", session: id }),
            h("div", { key: "zoom", style: pad }, h(TerminalZoom, { session: id })),
            h(SessionInfo, { key: "info", session: id }),
            h(SessionConfig, { key: "config", session: id }),
            h(
              "div",
              { key: "host-zoom", style: { ...pad, display: "flex", flexDirection: "column", alignItems: "flex-start", gap: "var(--g-2)" } },
              h("span", { className: "d-caps" }, "Zoom the page imposes"),
              h(Segmented, { label: "Host zoom", size: "sm", items: HOST_ZOOMS, value: hostZoom, onChange: setHostZoom }),
            ),
          ],
        }),
      ),
  );
}

// What an action's context says, in one line: its location, then what is at
// stake there (the entity, the selection, the pipeline, the Review pair), then
// the selected Run and its Projet.
function describe(context) {
  const parts = [context.location ?? "?"];
  const entity = context.entity;
  if (entity) parts.push(`${entity.kind} ${entity.id}${entity.status ? ` (${entity.status})` : ""}`);
  if (context.selected) parts.push(`selected ${context.selected.kind} ${context.selected.id}`);
  if (context.selection) parts.push(`${context.selection.ids.length} selected: ${context.selection.ids.join(", ")}`);
  if (context.pipeline) parts.push(`pipeline ${context.pipeline.id}`);
  if (context.review) parts.push(`review ${context.review.from} → ${context.review.to}`);
  parts.push(`Run ${context.run?.id ?? "none"}`, `Project ${context.project?.name ?? "none"}`);
  return parts.join(" · ");
}

// Command `test.show-context`, and every `Test <location>` action's: records
// the context it was called with, then tells the operator.
export async function showContext(context, host) {
  await fetch("/ext/test/commands", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(context),
  });
  host.notify(describe(context));
}

// Command `test.fail`: always throws, so the core's error toast shows.
export function failOnPurpose() {
  throw new Error("the test extension fails on purpose");
}

// #31 — one component per kind of contribution, at every location that
// accepts it.

function describeEntity(entity) {
  if (!entity) return "nothing";
  return `${entity.kind} ${entity.id ?? "(new)"}${entity.name ? ` · ${entity.name}` : ""}`;
}

// Tab of a detail (`node/detail`, `trigger/detail`, `skill/detail`): the entity
// it details, and its attributes.
export function EntityTab({ entity, context }) {
  const attributes = Object.entries(entity?.attributes ?? {});
  return h(
    "div",
    { style: box, "data-testid": `test-tab-${context.location.replace("/", "-")}` },
    h("p", null, describeEntity(entity)),
    attributes.length === 0
      ? h("p", { style: muted }, "No attribute.")
      : h("ul", null, attributes.map(([key, value]) => h("li", { key }, `${key} = ${value}`))),
  );
}

// Section (`project/edit`, `run/new`, `node/config`, `pipeline/config`,
// `run/info`): the entity of its host, and — where the host has a Save — the
// record of what the host saved, posted to `/ext/test/sections`.
export function EntitySection({ context, entity, form }) {
  const [saved, setSaved] = React.useState(null);
  // API 0.9: at `run/new`, provides `{ cle: <value> }` to the Run about to be
  // launched (sent as `extensions.test`); a value starting with `refuse` is
  // refused by the extension, and the form stays open with the reason.
  // API 0.22: opened by `openNewRun` with data for this extension, the draft
  // starts from it (`context.prefill`); no other extension's section sees it.
  const prefill = context.location === "run/new" ? context.prefill : undefined;
  const [value, setValue] = React.useState(() => prefill?.cle ?? "from-the-section");
  const provides = context.location === "run/new" && context.mode === "run";
  React.useEffect(() => {
    if (!form?.provide || !provides) return undefined;
    return form.provide(() => (value ? { cle: value } : undefined));
  }, [form, provides, value]);
  React.useEffect(() => {
    if (!form) return undefined;
    return form.onSave(async (what) => {
      await fetch("/ext/test/sections", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ location: context.location, saved: what }),
      });
      setSaved(what);
    });
  }, [form, context.location]);
  const extra =
    context.location === "run/new"
      ? `${context.mode} · pipeline ${context.pipeline ?? "none"} · name ${context.name || "none"} · prompt ${context.input ? `${context.input.length} chars` : "empty"}`
      : context.location === "node/config"
        ? `pipeline ${context.pipeline?.name}`
        : null;
  return h(
    "div",
    { style: { ...box, fontSize: "11px" }, "data-testid": `test-section-${context.location.replace("/", "-")}` },
    h("p", null, describeEntity(entity)),
    extra && h("p", { style: muted }, extra),
    form && h("p", { style: muted }, saved ? `Saved: ${describeEntity(saved)}` : "Takes part in Save"),
    prefill !== undefined &&
      h("p", { style: muted, "data-testid": "test-section-prefill" }, `Prefilled with ${JSON.stringify(prefill)}`),
    provides &&
      h(
        "label",
        { style: { ...muted, display: "flex", gap: "6px", alignItems: "center" } },
        "Provides cle =",
        h("input", {
          "data-testid": "test-section-provided",
          value,
          onChange: (e) => setValue(e.target.value),
          style: { fontSize: "11px", flex: 1 },
        }),
      ),
  );
}

// Decoration (`run/item`, `trigger/item`, `pipeline/item`, `canvas/node`,
// `port/item`, `review/file`, `statusbar`): a small boxed « T », the entity in
// its tooltip.
export function EntityBadge({ entity, context }) {
  return h(
    "span",
    {
      title: `Test · ${context.location} · ${describeEntity(entity)}`,
      "data-testid": `test-badge-${context.location.replace("/", "-")}`,
      style: {
        display: "inline-flex",
        alignItems: "center",
        border: "1px solid currentColor",
        borderRadius: "3px",
        padding: "0 3px",
        fontSize: "9px",
        lineHeight: "12px",
        opacity: 0.8,
      },
    },
    "T",
  );
}

// Decoration of `trigger/item`: the boxed « T » in a button, as an extension
// puts a gesture on a Trigger's row. A click stays off the row
// (it does not select the Trigger), counts in `data-clicks` and says so in a
// toast, so a test sees the button is reachable while the row's hover bar shows.
export function TriggerMark({ entity, context }) {
  const [clicks, setClicks] = React.useState(0);
  return h(
    "button",
    {
      type: "button",
      "data-testid": "test-trigger-action",
      "data-clicks": clicks,
      "aria-label": `Test action on ${entity?.name ?? "the trigger"}`,
      onClick: (e) => {
        e.stopPropagation();
        setClicks((n) => n + 1);
        toast(`Test action on ${entity?.name ?? "the trigger"}`);
      },
      onKeyDown: (e) => e.stopPropagation(),
      style: {
        display: "inline-flex",
        alignItems: "center",
        padding: 0,
        border: 0,
        background: "transparent",
        color: "inherit",
        cursor: "pointer",
      },
    },
    h(EntityBadge, { entity, context }),
  );
}

// Tab of `stats/tab` (0.9): the period of the Stats header and the Refresh
// count, as its context hands them over.
export function StatsTab({ context }) {
  const period = context?.period;
  return h(
    "div",
    { style: box, "data-testid": "test-extension-stats-tab" },
    h("h2", { style: { fontSize: "15px", fontWeight: 600 } }, "Stats of the test extension"),
    h("p", { "data-testid": "test-stats-period" }, period ? `Period: ${period.preset} (${period.bucket})` : "No period"),
    h("p", { "data-testid": "test-stats-refresh" }, `Refreshed ${context?.refresh ?? 0} times`),
  );
}

// Page of `settings/page` (0.9): its own rail entry in the Settings, and the
// path of its deep link.
export function SettingsPage({ context, navigate }) {
  const path = context?.path ?? "";
  return h(
    "div",
    { style: box, "data-testid": "test-extension-settings-page" },
    h("h2", { style: { fontSize: "15px", fontWeight: 600 } }, "Settings page of the test extension"),
    h("p", { "data-testid": "test-settings-page-path" }, `Path: ${path || "(none)"}`),
    h(
      "button",
      { type: "button", onClick: () => navigate("deep/inside"), style: { alignSelf: "flex-start", textDecoration: "underline" } },
      "Go to deep/inside",
    ),
  );
}

// Decoration of `runs/empty` (0.11): why the Runs list is empty, under the
// core's own text, and a Copy button through `copyToClipboard` of the shared
// module (0.11), which works over plain http too.
export function RunsEmptyNote() {
  const [copied, setCopied] = React.useState(null);
  return h(
    "div",
    { style: { ...muted, fontSize: "11px" }, "data-testid": "test-runs-empty-note" },
    h("p", null, "No Project yet: ask an admin for one"),
    h(
      "button",
      {
        type: "button",
        "data-testid": "test-runs-empty-copy",
        style: { textDecoration: "underline" },
        onClick: () => copyToClipboard("admin@example.test").then(setCopied),
      },
      copied === null ? "Copy the admin's address" : copied ? "Copied" : "Copy failed",
    ),
  );
}

// `run/item/people` (0.12): who has the row's Run open, live, as the kit's
// stack (0.20): each named as the extensions name its Actor (its kind
// otherwise), ringed as present, « Viewing now » in its tooltip.
export function PeopleMarks({ context }) {
  const presences = context.presences ?? [];
  const displays = useActorDisplays(presences.map((presence) => presence.actor));
  if (presences.length === 0) return null;
  return h(AvatarStack, {
    "data-testid": "test-people",
    people: presences.map((presence, i) => {
      const name = displays[i]?.name ?? presence.actor.id ?? presence.actor.kind;
      return { key: JSON.stringify(presence.actor), name, actor: presence.actor, present: true, title: `${name} · Viewing now` };
    }),
  });
}

// A decoration that throws: its boundary shows a mark in its place, and nothing
// else of the status bar moves. Also a tab of `stats/tab`: its tab says it
// failed, and the other tabs of Stats still open.
export function Exploding() {
  throw new Error("the test extension's exploding decoration");
}

// `topbar/account` (0.8): the signed-in Actor's avatar, after Settings, opening
// a menu with its name (`useCurrentActor` of the shared module) and Sign out
// (the sign-in page's cookie dropped, back to the sign-in page).
export function AccountMenu() {
  const current = useCurrentActor();
  const [open, setOpen] = React.useState(false);
  const name = current?.display?.name ?? current?.actor?.kind ?? "…";
  const signOut = () => {
    document.cookie = "pdo-test-token=; path=/; max-age=0";
    window.location.assign(`/ext/test/sign-in?next=${encodeURIComponent(window.location.hash ? "/" + window.location.hash : "/")}`);
  };
  return h(
    "span",
    { style: { position: "relative", display: "inline-flex" } },
    h(
      "button",
      {
        type: "button",
        "aria-label": `Account: ${name}`,
        "data-testid": "test-account-avatar",
        onClick: () => setOpen((o) => !o),
        style: { display: "inline-grid", padding: 0, border: "none", background: "transparent", cursor: "pointer" },
      },
      h(Avatar, { name, actor: current?.actor, size: 20 }),
    ),
    open &&
      h(
        "div",
        {
          role: "menu",
          "data-testid": "test-account-menu",
          style: {
            position: "absolute",
            right: 0,
            top: "30px",
            zIndex: 50,
            minWidth: "200px",
            padding: "8px",
            borderRadius: "8px",
            border: "1px solid rgba(127,127,127,.3)",
            background: "var(--bg-3, #1b1e24)",
            ...box,
          },
        },
        h("strong", { "data-testid": "test-account-name" }, name),
        h("span", { style: muted }, current?.actor?.kind ?? ""),
        h("button", { type: "button", role: "menuitem", onClick: signOut }, "Sign out"),
      ),
  );
}

// `review/composer` (0.14): a source of completion opened by `#`. Its tags
// depend on what is typed after `#` and on the Run under review.
const TAGS = ["bug", "perf", "review-later"];

export function suggestTags(query, context) {
  const run = context.run?.id ? [`run-${context.run.id.toLowerCase()}`] : [];
  return [...TAGS, ...run]
    .filter((tag) => tag.startsWith(query.toLowerCase()))
    .map((tag) => ({
      id: tag,
      label: tag,
      insert: `#${tag}`,
      hint: "tag",
      detail: tag.startsWith("run-") ? "this Run" : "Test",
    }));
}

// The render of an inserted `#tag`: a chip, in the text of the sent comment.
export function TagChip({ text }) {
  return h(
    "span",
    {
      "data-testid": "test-tag-chip",
      style: { padding: "0 4px", borderRadius: "4px", background: "rgba(34,197,94,.15)", color: "rgb(34,197,94)" },
    },
    text,
  );
}

// 0.18: the Source de recherche of the command field. Its notes, already
// filtered as the test extension's rights would; each opens the test page on
// its path. `query` is the text typed, `context.selected` the item selected.
// 0.20: each status carries its fixed shape (`todo`, `doing`, `review`,
// `done`), and one note has a 200-character title without a space.
const NOTES = [
  { id: "1", ref: "N-1", title: "Release checklist", meta: "Ops", tone: "done", shape: "done", status: "Done" },
  { id: "2", ref: "N-2", title: "Search index rebuild", meta: "Platform", tone: "running", shape: "doing", status: "In progress" },
  { id: "3", ref: "N-3", title: "Retry budget for flaky searches", meta: "Platform", tone: "skipped", shape: "todo", status: "To do" },
  { id: "4", ref: "N-4", title: "Upgrade notes for the next release", meta: "Ops", tone: "awaiting", shape: "review", status: "In review" },
  {
    id: "5",
    ref: "N-5",
    title: `Unbroken-${"segment-without-any-space-".repeat(8)}`.slice(0, 200),
    meta: "Platform › Long titles",
    tone: "skipped",
    shape: "todo",
    status: "To do",
  },
];

function noteResult(note) {
  return {
    id: note.id,
    ref: note.ref,
    title: note.title,
    meta: note.meta,
    status: { tone: note.tone, shape: note.shape, label: note.status },
    link: `#/ext/test/topbar/end/notes/${note.id}`,
  };
}

export function searchNotes(query) {
  const q = query.toLowerCase();
  return NOTES.filter((note) => note.title.toLowerCase().includes(q) || note.ref.toLowerCase().includes(q)).map(
    noteResult,
  );
}

// 0.20: people, each result carrying its avatar (its Actor's hue) and its
// role on the right.
const PEOPLE = [
  { id: "alice", name: "Alice Martin", role: "Administrator" },
  { id: "bob", name: "Bob Stone", role: "Maintainer" },
  { id: "carol", name: "Carol Dupont", role: "Member" },
];

export function searchPeople(query) {
  const q = query.toLowerCase();
  return PEOPLE.filter((p) => p.name.toLowerCase().includes(q) || p.id.includes(q)).map((p) => ({
    id: p.id,
    title: p.name,
    meta: p.role,
    avatar: { name: p.name, actor: { kind: "identified", namespace: "test", id: p.id } },
    link: `#/ext/test/topbar/end/people/${p.id}`,
  }));
}

// The same notes, answered after 3 s: a slow source.
export function searchSlowly(query) {
  return new Promise((resolve) => setTimeout(() => resolve(searchNotes(query)), 3000));
}

// A source that always fails: its error stays in its own group.
export function searchFailing() {
  return Promise.reject(new Error("index unavailable"));
}

// A source that answers 200 results with 200-character titles: the core keeps
// the first ones.
export function searchMany(query) {
  const long = `${query} ${"a-very-long-segment-without-any-space-".repeat(6)}`.slice(0, 200);
  return Array.from({ length: 200 }, (_, i) => ({
    id: String(i),
    ref: `M-${i + 1}`,
    title: `${i} ${long}`.slice(0, 200),
    meta: "Many",
    link: `#/ext/test/topbar/end/many/${i}`,
  }));
}
