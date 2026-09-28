// The UI bundle of the internal test extension: one component per UI slot.
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

// List filter of the Runs: keep the Runs a given caller started.
export function StartedByFilter({ filter, setFilter }) {
  const value = filter["test/started_by"] ?? "";
  return h(
    "label",
    { style: { display: "inline-flex", alignItems: "center", gap: "4px", fontSize: "12px" } },
    "Started by",
    h("input", {
      "aria-label": "Started by",
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
