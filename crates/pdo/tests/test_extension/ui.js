// The UI bundle of the internal test extension: one component per location it
// contributes to, and the function of its command.
// A plain ES module, no build step: `react` is the front's own, shared through
// its import map.
import React from "react";

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

// Page of `topbar/end`: the context it is opened with (the selected Run and its
// Projet) and the path of the deep link, in the shell's breadcrumb too.
export function TestPage({ context, navigate, shell }) {
  const run = context?.run ?? null;
  const project = context?.project ?? null;
  const path = context?.path ?? "";
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
      "button",
      { type: "button", onClick: () => navigate("deep/inside"), style: { alignSelf: "flex-start", textDecoration: "underline" } },
      "Go to deep/inside",
    ),
  );
}

// Command `test.show-context`: records the context it was called with, then
// tells the operator.
export async function showContext(context, host) {
  await fetch("/ext/test/commands", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(context),
  });
  host.notify(`Run ${context.run?.id ?? "none"} · Project ${context.project?.name ?? "none"}`);
}

// #31 — one component per kind of contribution, at every location that
// accepts it.

function describe(entity) {
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
    h("p", null, describe(entity)),
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
      ? `${context.mode} · pipeline ${context.pipeline ?? "none"}`
      : context.location === "node/config"
        ? `pipeline ${context.pipeline?.name}`
        : null;
  return h(
    "div",
    { style: { ...box, fontSize: "11px" }, "data-testid": `test-section-${context.location.replace("/", "-")}` },
    h("p", null, describe(entity)),
    extra && h("p", { style: muted }, extra),
    form && h("p", { style: muted }, saved ? `Saved: ${describe(saved)}` : "Takes part in Save"),
  );
}

// Decoration (`run/item`, `trigger/item`, `pipeline/item`, `canvas/node`,
// `port/item`, `review/file`, `statusbar`): a small boxed « T », the entity in
// its tooltip.
export function EntityBadge({ entity, context }) {
  return h(
    "span",
    {
      title: `Test · ${context.location} · ${describe(entity)}`,
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

// A decoration that throws: its boundary shows a mark in its place, and nothing
// else of the status bar moves.
export function Exploding() {
  throw new Error("the test extension's exploding decoration");
}
