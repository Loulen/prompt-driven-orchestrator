// The UI bundle of test extension B (hosted.rs): its contributions to the
// locations extension A hosts. A plain ES module, no build step.
import React from "react";

const h = React.createElement;

const box = {
  fontSize: "12px",
  borderLeft: "2px solid currentColor",
  paddingLeft: "8px",
};

// Section of `a/panneau`: the context A handed it.
export function BPanel({ extension, context }) {
  return h(
    "div",
    { style: box, "data-testid": `b-panel-${context.item?.id ?? "none"}` },
    `${extension.id} panel · ${context.location} · ${context.item?.title ?? "no item"}`,
  );
}

// Second section of `a/panneau`, while B contributes two.
export function BSecondPanel({ context }) {
  return h(
    "div",
    { style: box, "data-testid": `b-second-panel-${context.item?.id ?? "none"}` },
    `B second panel · ${context.item?.id ?? "no item"}`,
  );
}

// Command `b.ping`, the action of `a/actions`.
export function ping(context, host) {
  host.notify(`B pinged from ${context.location} on ${context.item?.title ?? "no item"}`);
}
