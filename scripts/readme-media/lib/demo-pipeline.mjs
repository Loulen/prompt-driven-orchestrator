// The demo pipeline — the ONLY pipeline any README media shows (design rule
// « one pipeline only »). Its YAML lives in fixture/pipelines/ and is installed
// into the demo HOME's library; this is its run snapshot (`run_started`'s
// `node_defs` / `edges`), as the mocked history freezes it.

export const DEMO_PIPELINE = {
  id: "implement-review",
  name: "implement-review",
  nodeDefs: [
    { id: "start", name: "Start", node_type: "start", view_x: 300, view_y: 0, inputs: [], outputs: [{ name: "user_prompt", side: "bottom" }] },
    { id: "implementer", name: "implementer", node_type: "agent", isolated_worktree: false, view_x: 300, view_y: 160, inputs: [{ name: "task", side: "top" }], outputs: [{ name: "code", side: "bottom" }] },
    {
      id: "reviewer",
      name: "reviewer",
      node_type: "agent",
      isolated_worktree: false,
      view_x: 300,
      view_y: 340,
      inputs: [{ name: "code", side: "top" }],
      outputs: [
        { name: "review", side: "bottom" },
        { name: "screenshots", side: "right" },
      ],
    },
    { id: "end", name: "End", node_type: "end", view_x: 300, view_y: 520, inputs: [{ name: "result", side: "top" }], outputs: [] },
  ],
  edges: [
    { source_node: "start", source_port: "user_prompt", target_node: "implementer", target_port: "task" },
    { source_node: "implementer", source_port: "code", target_node: "reviewer", target_port: "code" },
    { source_node: "reviewer", source_port: "review", target_node: "end", target_port: "result" },
  ],
};
