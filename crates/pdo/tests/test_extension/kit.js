// The UI bundle of the design-system test extension (ADR-0091): a "Deploy"
// page drawn with nothing but the primitives of `@pdo/extension-ui` and the
// tokens of the shared stylesheet, the classes of the design system included.
// Its own styles are layout only (flex, grid, gaps), in tokens.
// A plain ES module, no build step: `react` is the front's own, shared through
// its import map.
import React from "react";
import {
  Avatar,
  Banner,
  Button,
  Checkbox,
  Chip,
  ConfirmDialog,
  EmptyState,
  Field,
  IconButton,
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
const Rocket = icon("M5 19l3-3", "M9 15l-3-3 6-7c2-2 5-2 7-2 0 2 0 5-2 7l-7 6z", "M14 9h.01");
const Refresh = icon("M20 12a8 8 0 1 1-2.3-5.7L20 8", "M20 3v5h-5");
const Plus = icon("M12 5v14", "M5 12h14");
const Filter = icon("M3 5h18l-7 8v6l-4-2v-4z");
const More = icon("M5 12h.01", "M12 12h.01", "M19 12h.01");
const Trash = icon("M4 7h16", "M9 7V4h6v3", "M6 7l1 13h10l1-13");
const Undo = icon("M9 14L4 9l5-5", "M4 9h10a6 6 0 0 1 0 12h-3");
const Copy = icon("M9 9h11v11H9z", "M5 15H4V4h11v1");
const Server = icon("M4 4h16v6H4z", "M4 14h16v6H4z", "M8 7h.01", "M8 17h.01");
const List = icon("M8 6h12", "M8 12h12", "M8 18h12", "M4 6h.01", "M4 12h.01", "M4 18h.01");
const Gear = icon("M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M19 12h2", "M3 12h2", "M12 3v2", "M12 19v2");
const Branch = icon("M6 3v12", "M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M18 9a9 9 0 0 1-9 9");

const LONG =
  "preview-shop-search-result-highlighting-for-long-product-names-in-the-catalogue-grid-with-fallback-to-sku-and-brand-when-the-title-is-missing-or-truncated-by-the-merchant-feed-import-job-v2";

const ENVIRONMENTS = [
  { id: "prod", name: "production", tone: "done", status: "Healthy", branch: "main", commit: "4f2a9c1", region: "eu-west-3", replicas: 6, auto: false, strategy: "blue-green", age: "2h" },
  { id: "staging", name: "staging", tone: "running", status: "Deploying", branch: "main", commit: "b81e07d", region: "eu-west-3", replicas: 2, auto: true, strategy: "rolling", age: "1m" },
  { id: "qa", name: "qa", tone: "failed", status: "Failed", branch: "release/2.14", commit: "9c3d5e2", region: "eu-west-1", replicas: 1, auto: true, strategy: "recreate", age: "14m", reason: "Health check timed out after 120 s on /api/ready" },
  { id: "pr-12", name: "preview-shop-12", tone: "awaiting", status: "Awaiting approval", branch: "feature/SHOP-12-product-search", commit: "e17a442", region: "eu-west-3", replicas: 1, auto: true, strategy: "rolling", age: "32m", reason: "Approval by a Maintainer" },
  { id: "long", name: LONG, tone: "blocked", status: "Degraded", branch: "feature/SHOP-9-search-result-highlighting-for-long-product-names", commit: "0d9b1f3", region: "eu-central-1", replicas: 3, auto: false, strategy: "rolling", age: "3d", reason: "1 of 3 replicas restarting" },
  { id: "sandbox", name: "sandbox", tone: "skipped", status: "Paused", branch: "main", commit: "77ac0e9", region: "eu-west-3", replicas: 1, auto: false, strategy: "recreate", age: "12d" },
];
const STRATEGIES = [
  { value: "rolling", label: "Rolling" },
  { value: "blue-green", label: "Blue/green" },
  { value: "recreate", label: "Recreate" },
];
// A choice that fills its row (`fill`), its labels long enough to end in an ellipsis.
const WINDOWS = [
  { value: "any", label: "Any time" },
  { value: "office", label: "Office hours, Monday to Friday" },
  { value: "weekend", label: "Weekends only" },
];
const BRANCHES = ["main", "release/2.14", "release/2.13", "feature/SHOP-12-product-search"];
const PEOPLE = [
  { by: "Alice Martin", kind: "human" },
  { by: "implement-review", kind: "agent" },
  { by: "nightly-sync", kind: "trigger" },
  { by: "Bob Durand", kind: "human" },
];

/** 50 deploys of an environment, newest first. */
function deploysOf(env) {
  return Array.from({ length: 50 }, (_, i) => {
    const who = PEOPLE[i % PEOPLE.length];
    const failed = i % 7 === 3;
    return {
      id: `${env.id}-${i}`,
      version: `v2.${14 - Math.floor(i / 4)}.${(50 - i) % 10}`,
      tone: i === 0 ? env.tone : failed ? "failed" : "done",
      status: i === 0 ? env.status : failed ? "Failed" : "Deployed",
      ...who,
      age: `${i + 1}d`,
    };
  });
}

// Layout, in tokens.
const row = (gap = "var(--g-3)") => ({ display: "flex", alignItems: "center", gap, minWidth: 0 });
const ellipsis = { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0 };

function DeployRow({ d }) {
  return h(
    "div",
    { className: "d-row", "data-testid": "kit-deploy-row" },
    h(Avatar, { name: d.by, kind: d.kind, size: 16 }),
    h("span", { style: { ...ellipsis, flex: "1 1 auto" }, className: d.kind === "human" ? undefined : "d-mono" }, d.by),
    h(Chip, { mono: true }, d.version),
    h(Chip, { tone: d.tone }, d.status),
    h("span", { className: "d-meta d-mono" }, d.age),
  );
}

function Overview({ env }) {
  return h(
    React.Fragment,
    null,
    h(
      Section,
      { title: "Target", aside: h(IconButton, { icon: Copy, label: "Copy URL", size: "sm", onClick: () => toast("URL copied", "success") }) },
      h(KeyValue, {
        rows: [
          { k: "Branch", v: env.branch, mono: true },
          { k: "Commit", v: env.commit, mono: true },
          { k: "Region", v: env.region },
          { k: "Replicas", v: String(env.replicas) },
          { k: "Strategy", v: STRATEGIES.find((s) => s.value === env.strategy).label },
        ],
      }),
    ),
    h(Section, { title: "Recent deploys", count: 50, flush: true }, deploysOf(env).slice(0, 5).map((d) => h(DeployRow, { key: d.id, d }))),
  );
}

function History({ env }) {
  const [who, setWho] = React.useState("all");
  const all = deploysOf(env);
  const list = who === "all" ? all : all.filter((d) => d.kind === who);
  return h(
    Section,
    {
      title: "Deploys",
      count: list.length,
      flush: true,
      aside: h(Segmented, {
        size: "sm",
        label: "Deployed by",
        value: who,
        onChange: setWho,
        items: [
          { value: "all", label: "All" },
          { value: "human", label: "People" },
          { value: "agent", label: "Agents" },
          { value: "trigger", label: "Triggers" },
        ],
      }),
    },
    list.length === 0 ? h(EmptyState, { icon: Rocket, layout: "row" }, "No deploys") : list.map((d) => h(DeployRow, { key: d.id, d })),
  );
}

function Settings({ env, onSave }) {
  const [draft, setDraft] = React.useState(env);
  const [replicas, setReplicas] = React.useState(String(env.replicas));
  const [notify, setNotify] = React.useState(true);
  const [deployWindow, setDeployWindow] = React.useState("office");
  const bad = !/^\d+$/.test(replicas) || +replicas < 1 || +replicas > 20;
  return h(
    Section,
    { title: "Configuration" },
    h(
      "div",
      { style: { maxWidth: "520px" }, "data-testid": "kit-settings" },
      h(Field, { label: "Name", error: draft.name.trim() ? null : "Required" }, h(TextInput, { value: draft.name, onChange: (e) => setDraft({ ...draft, name: e.target.value }) })),
      h(
        Field,
        { label: "Branch" },
        h(Select, { label: "Branch", mono: true, icon: Branch, value: draft.branch, onChange: (v) => setDraft({ ...draft, branch: v }), options: BRANCHES.map((b) => ({ value: b, label: b })) }),
      ),
      h(Field, { label: "Strategy" }, h(Segmented, { label: "Strategy", value: draft.strategy, onChange: (v) => setDraft({ ...draft, strategy: v }), items: STRATEGIES })),
      h(Field, { label: "Window" }, h(Segmented, { label: "Deploy window", fill: true, value: deployWindow, onChange: setDeployWindow, items: WINDOWS })),
      h(
        Field,
        { label: "Replicas", help: "1 to 20", error: bad ? "Must be 1 to 20" : null },
        h(TextInput, { mono: true, inputMode: "numeric", value: replicas, onChange: (e) => setReplicas(e.target.value), style: { maxWidth: 80 } }),
      ),
      h(Field, { label: "Auto-deploy" }, h(Switch, { label: "Auto-deploy", checked: draft.auto, onChange: (v) => setDraft({ ...draft, auto: v }) })),
      h(Field, { label: "Notify" }, h(Checkbox, { label: "On failure", checked: notify, onChange: setNotify }, "On failure")),
      h(
        "div",
        { style: { ...row("var(--g-3)"), justifyContent: "flex-end", paddingTop: "var(--g-4)" } },
        h(Button, { onClick: () => setDraft(env) }, "Reset"),
        h(Button, { tone: "primary", disabled: bad || !draft.name.trim(), onClick: () => onSave({ ...draft, replicas: +replicas }) }, "Save"),
      ),
    ),
  );
}

function DeployModal({ env, onClose, onDeploy }) {
  const [version, setVersion] = React.useState("v2.14.9");
  return h(
    Modal,
    {
      title: `Deploy ${env.name}`,
      onClose,
      onSubmit: () => onDeploy(version),
      "data-testid": "kit-deploy-modal",
      footerNote: h("span", { className: "d-mono" }, `${env.branch} · ${env.commit}`),
      actions: h(
        React.Fragment,
        null,
        h(Button, { onClick: onClose }, "Cancel"),
        h(Button, { tone: "primary", icon: Rocket, onClick: () => onDeploy(version) }, "Deploy"),
      ),
    },
    h(
      Field,
      { label: "Version" },
      h(Select, {
        label: "Version",
        mono: true,
        value: version,
        onChange: setVersion,
        options: ["v2.14.9", "v2.14.8", "v2.13.2"].map((v, i) => ({ value: v, label: v, hint: i === 0 ? "latest" : undefined })),
      }),
    ),
    h(Field, { label: "Note" }, h(TextArea, { rows: 3, placeholder: "Note", "data-autofocus": "" })),
  );
}

// Page of `topbar/start`: the environments, one of them open.
export function KitPage() {
  const [envs, setEnvs] = React.useState(ENVIRONMENTS);
  const [selected, setSelected] = React.useState("qa");
  const [tab, setTab] = React.useState("overview");
  const [only, setOnly] = React.useState([]);
  const [modal, setModal] = React.useState(null);
  const env = envs.find((e) => e.id === selected) ?? envs[0];
  const statuses = [...new Set(ENVIRONMENTS.map((e) => e.status))];
  const shown = envs.filter((e) => only.length === 0 || only.includes(e.status));

  const deploy = (version) => {
    setModal(null);
    setEnvs((list) => list.map((e) => (e.id === env.id ? { ...e, tone: "running", status: "Deploying", age: "now", reason: undefined } : e)));
    toast(`Deploying ${env.name} ${version}`, "info", { actionLabel: "View", onAction: () => setTab("history") });
  };
  const remove = () => {
    setModal(null);
    setEnvs((list) => list.filter((e) => e.id !== env.id));
    setSelected(envs.find((e) => e.id !== env.id)?.id ?? "");
    toast(`Deleted ${env.name}`, "success");
  };
  const menu = env
    ? [
        { label: "Roll back", icon: Undo, disabled: env.tone === "running" },
        { label: "Copy URL", icon: Copy, kbd: "Ctrl+C", onSelect: () => toast("URL copied", "success") },
        { kind: "separator" },
        { label: "Auto-deploy", checked: env.auto, onSelect: () => setEnvs((list) => list.map((e) => (e.id === env.id ? { ...e, auto: !e.auto } : e))) },
        { kind: "separator" },
        { label: "Delete", icon: Trash, danger: true, onSelect: () => setModal("delete") },
      ]
    : [];

  const side = h(
    "aside",
    { style: { width: "280px", flex: "none", borderRight: "1px solid var(--line)", background: "var(--surface-1)", overflowY: "auto" } },
    h(
      Section,
      {
        title: "Environments",
        count: shown.length,
        flush: true,
        aside: h(
          Popover,
          {
            label: "Filters",
            align: "end",
            trigger: ({ open, toggle }) => h(IconButton, { icon: Filter, label: "Filters", size: "sm", pressed: open || only.length > 0, onClick: toggle }),
          },
          h(
            "div",
            { style: { display: "flex", flexDirection: "column", gap: "var(--g-2)", padding: "var(--g-2)" } },
            statuses.map((s) =>
              h(Checkbox, { key: s, label: s, checked: only.includes(s), onChange: (on) => setOnly((list) => (on ? [...list, s] : list.filter((x) => x !== s))) }, s),
            ),
          ),
        ),
      },
      only.length > 0 &&
        h(
          "div",
          { style: { ...row("var(--g-2)"), flexWrap: "wrap", padding: "var(--g-1) var(--pad-x)" } },
          only.map((s) => h(Chip, { key: s, removeLabel: `Clear ${s}`, onRemove: () => setOnly((list) => list.filter((x) => x !== s)) }, s)),
        ),
      shown.length === 0 && h(EmptyState, { icon: Filter, action: h(Button, { size: "sm", onClick: () => setOnly([]) }, "Clear filters") }, "No match"),
      h(
        "div",
        { role: "listbox", "aria-label": "Environments" },
        shown.map((e) =>
          h(
            "div",
            {
              key: e.id,
              role: "option",
              "aria-selected": e.id === env?.id,
              tabIndex: 0,
              title: e.name,
              className: "d-row",
              style: { height: "var(--h-row-2)", cursor: "pointer" },
              onClick: () => setSelected(e.id),
              onKeyDown: (ev) => ev.key === "Enter" && setSelected(e.id),
            },
            h(StatusIcon, { kind: "tone", status: e.tone, label: e.status }),
            h(
              "span",
              { style: { display: "flex", flexDirection: "column", minWidth: 0, flex: "1 1 auto" } },
              h("span", { style: ellipsis }, e.name),
              h("span", { className: "d-meta", style: ellipsis }, `${e.status} · ${e.branch}`),
            ),
            h("span", { className: "d-meta d-mono" }, e.age),
          ),
        ),
      ),
    ),
  );

  const main = !env
    ? h(EmptyState, { icon: Server, action: h(Button, { size: "sm", icon: Plus }, "New environment") }, "No environments")
    : h(
        "div",
        { style: { display: "flex", flexDirection: "column", minWidth: 0, flex: "1 1 auto" } },
        h(
          "div",
          { className: "d-bar" },
          h(Tabs, {
            label: "Environment",
            value: tab,
            onChange: setTab,
            items: [
              { value: "overview", label: "Overview", icon: Server },
              { value: "history", label: "History", icon: List, count: 50 },
              { value: "settings", label: "Settings", icon: Gear },
            ],
          }),
          h("span", { style: { flex: "1 1 auto" } }),
          h(IconButton, { icon: Refresh, label: "Refresh", kbd: "R", onClick: () => toast("Refreshed") }),
          h(Tooltip, { label: "Deploy", kbd: "D" }, h(Button, { tone: "primary", icon: Rocket, disabled: env.tone === "running", onClick: () => setModal("deploy") }, "Deploy")),
          h(Menu, {
            label: env.name,
            align: "end",
            entries: menu,
            trigger: ({ open, toggle }) => h(IconButton, { icon: More, label: "More", pressed: open, onClick: toggle }),
          }),
        ),
        (env.tone === "failed" || env.tone === "awaiting" || env.tone === "blocked") &&
          h(Banner, {
            tone: env.tone,
            title: env.status,
            text: env.reason,
            actions:
              env.tone === "failed"
                ? h(Button, { size: "sm", icon: Undo, onClick: () => deploy("v2.14.9") }, "Retry")
                : env.tone === "awaiting"
                  ? h(Button, { size: "sm", tone: "primary" }, "Approve")
                  : undefined,
          }),
        h(
          "div",
          { style: { overflowY: "auto", padding: "var(--g-5) var(--g-6)", display: "flex", flexDirection: "column", gap: "var(--g-4)", minWidth: 0 } },
          h(
            "div",
            { style: row("var(--g-4)") },
            h("h2", { className: "d-frame__title", title: env.name, "data-testid": "kit-env-title" }, env.name),
            h(Chip, { tone: env.tone }, env.status),
          ),
          tab === "overview" && h(Overview, { env }),
          tab === "history" && h(History, { env }),
          tab === "settings" &&
            h(Settings, {
              key: env.id,
              env,
              onSave: (next) => {
                setEnvs((list) => list.map((e) => (e.id === env.id ? next : e)));
                toast(`Saved ${next.name}`, "success");
              },
            }),
        ),
      );

  return h(
    "div",
    { "data-testid": "kit-page", style: { display: "flex", height: "100%", minHeight: 0, background: "var(--surface-0)", color: "var(--text)" } },
    side,
    main,
    modal === "deploy" && env && h(DeployModal, { env, onClose: () => setModal(null), onDeploy: deploy }),
    modal === "delete" &&
      env &&
      h(ConfirmDialog, {
        title: `Delete environment ${env.name}?`,
        consequence: "Removes 50 past deploys",
        target: env.name,
        actionLabel: "Delete",
        danger: true,
        onClose: () => setModal(null),
        onConfirm: remove,
      }),
  );
}
