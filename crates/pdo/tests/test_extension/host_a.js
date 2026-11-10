// The UI bundle of test extension A (hosted.rs): a page hosting two locations,
// `a/panneau` and `a/actions`, rendered through the shared module
// `@pdo/extension-ui`. A plain ES module, no build step: `react` and
// `@pdo/extension-ui` are the front's own, shared through its import map.
import React from "react";
import { HostedContributions, useHostedContributions, version } from "@pdo/extension-ui";

const h = React.createElement;

const ITEMS = [
  { id: "item-1", title: "First item" },
  { id: "item-2", title: "Second item" },
];

const card = {
  display: "flex",
  flexDirection: "column",
  gap: "6px",
  border: "1px solid rgba(127,127,127,0.35)",
  borderRadius: "6px",
  padding: "10px 12px",
};

function Item({ item }) {
  const context = React.useMemo(() => ({ item }), [item]);
  const count = useHostedContributions("a/panneau", context).length;
  return h(
    "section",
    { style: card, "data-testid": `host-a-${item.id}` },
    h("h3", { style: { fontSize: "13px", fontWeight: 600 } }, item.title),
    h("p", { style: { opacity: 0.7, fontSize: "11px" }, "data-testid": `host-a-${item.id}-count` }, `Contributions: ${count}`),
    h(HostedContributions, { location: "a/panneau", context }),
  );
}

// Page of `topbar/end`: its items, B's sections under each, B's actions in the
// shell's header.
export function HostPage({ shell }) {
  const context = React.useMemo(() => ({ item: ITEMS[0] }), []);
  React.useEffect(() => {
    shell?.setActions(
      h(
        "div",
        { style: { display: "flex", gap: "6px" }, "data-testid": "host-a-actions" },
        h(HostedContributions, { location: "a/actions", context }),
      ),
    );
    return () => shell?.setActions(null);
  }, [shell, context]);
  return h(
    "div",
    {
      style: { display: "flex", flexDirection: "column", gap: "12px", padding: "20px 28px", width: "100%", fontSize: "13px" },
      "data-testid": "host-a-page",
    },
    h("p", { style: { opacity: 0.7 } }, `Host A, shared module ${version}`),
    ITEMS.map((item) => h(Item, { key: item.id, item })),
  );
}
