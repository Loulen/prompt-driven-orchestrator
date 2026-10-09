// The UI bundle of the internal test extension: one component per location it
// contributes to, and the function of its command.
// A plain ES module, no build step: `react` is the front's own, shared through
// its import map.
import React from "react";
import { copyToClipboard, openProbedSocket, useActorDisplay, useCurrentActor } from "@pdo/extension-ui";

const h = React.createElement;

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
// state of its probed socket.
export function TestPage({ context, navigate, shell }) {
  const run = context?.run ?? null;
  const project = context?.project ?? null;
  const path = context?.path ?? "";
  const stream = useProbedStream();
  React.useEffect(() => {
    shell?.setBreadcrumb(path ? path.split("/") : []);
  }, [shell, path]);
  return h(
    "div",
    { style: { ...box, padding: "20px 28px" }, "data-testid": "test-extension-page" },
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
  );
}

// What an action's context says, in one line: its location, then what is at
// stake there (the entity, the selection, the pipeline, the Review pair), then
// the selected Run and its Projet.
function describe(context) {
  const parts = [context.location ?? "?"];
  const entity = context.entity;
  if (entity) parts.push(`${entity.kind} ${entity.id}${entity.status ? ` (${entity.status})` : ""}`);
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
  const [value, setValue] = React.useState("from-the-section");
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

// `run/item/people` (0.12): one mark per Presence of the row's Run, live, each
// named as the extensions name its Actor (its kind otherwise).
export function PeopleMarks({ context }) {
  const presences = context.presences ?? [];
  if (presences.length === 0) return null;
  return h(
    "span",
    { style: { display: "inline-flex", gap: "2px" }, "data-testid": "test-people" },
    presences.map((presence) => h(PresenceMark, { key: JSON.stringify(presence.actor), presence })),
  );
}

function PresenceMark({ presence }) {
  const display = useActorDisplay(presence.actor);
  const name = display?.name ?? presence.actor.id ?? presence.actor.kind;
  return h(
    "span",
    {
      "data-testid": "test-presence",
      title: `${name}, here since ${presence.since}`,
      style: { border: "1px solid currentColor", borderRadius: "8px", padding: "0 4px", fontSize: "10px" },
    },
    name,
  );
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
  const initials = name.slice(0, 2).toUpperCase();
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
        style: {
          width: "24px",
          height: "24px",
          borderRadius: "999px",
          border: "none",
          background: "#f97316",
          color: "#fff",
          fontSize: "9px",
          fontWeight: 600,
          cursor: "pointer",
        },
      },
      initials,
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
