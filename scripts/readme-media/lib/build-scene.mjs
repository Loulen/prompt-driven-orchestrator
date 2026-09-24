// The scenes that BUILD on the canvas (pipelines, routing; ADR-0074 §6, #882)
// start from their target and come back to it. A scene declares what its
// gestures draw; the two pure functions here are the whole contract:
//
//   - startState(target, drawn): the target minus what the scene draws — the
//     pipeline the tool installs before the take;
//   - gestureDrift(saved, target): where what the gestures saved differs
//     VISIBLY from the target (node positions, edges, conditions, anchors,
//     routes, label positions, loop regions). The scene records it in the
//     manifest as a warning, then reinstalls the target off camera: the final
//     shot and the poster are the target, whatever the gestures produced.
//
// `drawn` names nodes by id and edges as `"<source node>→<target node>"`:
//
//   { nodes: ["implementer"] }                         // placed: its edges go with it
//   { edges: ["reviewer→implementer"] }                // drawn: the loop region it closes goes with it
//   { conditions: ["reviewer→end"] }                   // set: the edge stays, its `when` and pill go
//
// Pure: pipelines in (parsed YAML), pipelines or strings out.

/** An edge's key in `drawn`: its source node → its target node. */
export function edgeKey(edge) {
  return `${edge.source.node}→${edge.target.node}`;
}

/**
 * The pipeline a building scene starts from: `target` without the nodes, edges
 * and conditions its gestures draw. A removed node takes every edge touching
 * it; a loop region whose cycle no longer closes goes too (the editor
 * materializes it again when the gesture closes the cycle). Throws when `drawn`
 * names something the target does not have: a scene cannot draw what the
 * maintainer did not.
 */
export function startState(target, drawn = {}) {
  const pipeline = structuredClone(target);
  const nodes = new Set(drawn.nodes ?? []);
  const edges = new Set(drawn.edges ?? []);
  const conditions = new Set(drawn.conditions ?? []);
  const ids = new Set((pipeline.nodes ?? []).map((n) => n.id));
  const keys = new Set((pipeline.edges ?? []).map(edgeKey));
  for (const id of nodes) if (!ids.has(id)) throw new Error(`the target has no node \`${id}\``);
  for (const key of [...edges, ...conditions]) if (!keys.has(key)) throw new Error(`the target has no edge \`${key}\``);
  for (const key of conditions) {
    if (pipeline.edges.find((e) => edgeKey(e) === key).when === undefined) throw new Error(`the target's edge \`${key}\` has no condition`);
  }

  pipeline.nodes = (pipeline.nodes ?? []).filter((n) => !nodes.has(n.id));
  pipeline.edges = (pipeline.edges ?? []).filter((e) => !edges.has(edgeKey(e)) && !nodes.has(e.source.node) && !nodes.has(e.target.node));
  for (const edge of pipeline.edges) {
    if (!conditions.has(edgeKey(edge))) continue;
    delete edge.when;
    delete edge.condition_label_pos;
  }
  if (pipeline.loops) {
    const loops = pipeline.loops
      .map((loop) => ({ ...loop, members: loop.members.filter((m) => !nodes.has(m)) }))
      .filter((loop) => closesCycle(loop.members, pipeline.edges));
    if (loops.length > 0) pipeline.loops = loops;
    else delete pipeline.loops;
  }
  return pipeline;
}

/** Whether the edges between `members` still form a cycle. */
function closesCycle(members, edges) {
  const inside = new Set(members);
  const next = new Map(members.map((m) => [m, []]));
  for (const e of edges) if (inside.has(e.source.node) && inside.has(e.target.node)) next.get(e.source.node).push(e.target.node);
  const state = new Map();
  const visit = (node) => {
    state.set(node, "open");
    for (const to of next.get(node)) {
      if (state.get(to) === "open") return true;
      if (!state.has(to) && visit(to)) return true;
    }
    state.set(node, "done");
    return false;
  };
  return members.some((m) => !state.has(m) && visit(m));
}

// ---- drift -------------------------------------------------------------------

/** Flow px under which two positions read the same on the canvas. */
export const DRIFT_TOLERANCE = 6;

const fmt = (p) => (p ? `(${p.x}, ${p.y})` : "none");
const far = (a, b, tolerance) => Math.hypot(a.x - b.x, a.y - b.y) > tolerance;

/** The largest distance from a point of one polyline to the other (Hausdorff). */
function routeDistance(a, b) {
  const toSegment = (p, s, t) => {
    const dx = t.x - s.x;
    const dy = t.y - s.y;
    const len = dx * dx + dy * dy;
    const k = len === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - s.x) * dx + (p.y - s.y) * dy) / len));
    return Math.hypot(p.x - (s.x + k * dx), p.y - (s.y + k * dy));
  };
  const toLine = (p, line) => (line.length === 1 ? Math.hypot(p.x - line[0].x, p.y - line[0].y) : Math.min(...line.slice(1).map((t, i) => toSegment(p, line[i], t))));
  return Math.max(...a.map((p) => toLine(p, b)), ...b.map((p) => toLine(p, a)));
}

/** Whether a card shows the isolation marker (the editor's `isNodeIsolated`:
 *  an agent is isolated unless it says otherwise, a merge always). */
function isolated(node) {
  if (node.type === "merge") return true;
  return node.isolated_worktree ?? node.type === "agent";
}

/** A card's size when it was not measured (flow px). */
const CARD = { width: 160, height: 35 };

/** A point on a card's border: its anchor, or the middle of `side`. */
function borderPoint(node, size, anchor, side) {
  const { x, y } = node?.view ?? { x: 0, y: 0 };
  const s = anchor?.side ?? side;
  const along = anchor?.offset ?? (s === "top" || s === "bottom" ? size.width : size.height) / 2;
  if (s === "top") return { x: x + along, y };
  if (s === "bottom") return { x: x + along, y: y + size.height };
  if (s === "left") return { x, y: y + along };
  return { x: x + size.width, y: y + along };
}

/** The perpendicular leg the canvas draws out of and into a card (flow px):
 *  the editor's `landingLeg(WIRING_GRID_STEP)`, one 40 px grid cell. */
export const LANDING_LEG = 40;

const outward = { left: { x: -1, y: 0 }, right: { x: 1, y: 0 }, top: { x: 0, y: -1 }, bottom: { x: 0, y: 1 } };
const same = (a, b) => Math.abs(a - b) < 1e-6;

/** Merge straight runs (a reversal on one axis included): what the canvas'
 *  collinear merge does. The first and last point stay. */
function dedupeCollinear(points) {
  const out = [];
  for (const p of points) {
    const last = out[out.length - 1];
    if (last && same(last.x, p.x) && same(last.y, p.y)) continue;
    out.push({ ...p });
    while (out.length >= 3) {
      const [a, b, c] = out.slice(-3);
      if (!((same(a.y, b.y) && same(b.y, c.y)) || (same(a.x, b.x) && same(b.x, c.x)))) break;
      out.splice(out.length - 2, 1);
    }
  }
  return out;
}

/** Insert a bend into each diagonal, carrying on the way the wire goes unless
 *  that reverses it (the canvas' `squareUp`). */
function squareUp(points) {
  const out = [{ ...points[0] }];
  for (const b of points.slice(1)) {
    const a = out[out.length - 1];
    if (!same(a.x, b.x) && !same(a.y, b.y)) {
      const before = out[out.length - 2];
      const horizontal = before ? same(before.y, a.y) : false;
      const reverses = before ? (horizontal ? Math.sign(b.x - a.x) === -Math.sign(a.x - before.x) : Math.sign(b.y - a.y) === -Math.sign(a.y - before.y)) : false;
      out.push(horizontal !== reverses ? { x: b.x, y: a.y } : { x: a.x, y: b.y });
    }
    out.push({ ...b });
  }
  return out;
}

/**
 * The wire the canvas draws for `[source anchor, ...waypoints, target anchor]`
 * (flow px): a leg straight out of `sourceSide`, the waypoints squared up, a leg
 * straight into `targetSide` — the editor's `enforcePerpendicularEnds`, without
 * its detour around the target card. Two waypoint lists that draw the same wire
 * give the same route; the scenes' cursor follows it when it draws an edge.
 */
export function drawnRoute(points, sourceSide, targetSide, leg = LANDING_LEG) {
  const src = points[0];
  const tgt = points[points.length - 1];
  const approach = (p, side) => ({ x: p.x + outward[side].x * leg, y: p.y + outward[side].y * leg });
  const onEnd = (p) => (same(p.x, src.x) && same(p.y, src.y)) || (same(p.x, tgt.x) && same(p.y, tgt.y));
  const mid = dedupeCollinear([approach(src, sourceSide), ...points.slice(1, -1).filter((p) => !onEnd(p)), approach(tgt, targetSide)]);
  const squared = squareUp([src, ...mid, tgt]);
  return [squared[0], ...dedupeCollinear(squared.slice(1, -1)), squared[squared.length - 1]];
}

/**
 * The cursor's path to draw an edge whose wire is `route` (`drawnRoute`):
 * pressed at `press` (on the source rim), along the wire's corners, released at
 * `drop` (on the target, where it anchors). Straight runs are one move — a
 * reversal the canvas merges away is never shown — and every move is on one
 * axis, so the editor's grid trace commits the same bends on every take.
 */
export function cursorPath(route, press, drop) {
  return dedupeCollinear([press, ...route.slice(1, -1), drop]);
}

/** An edge's route as the canvas draws it, in flow px (`drawnRoute`): from its
 *  source anchor, through its pinned waypoints, to its target anchor. The auto
 *  router's bends are not modelled: two auto edges between the same anchors
 *  read the same. */
function route(edge, pipeline, sizeOf) {
  const node = (id) => (pipeline.nodes ?? []).find((n) => n.id === id);
  const from = node(edge.source.node);
  const to = node(edge.target.node);
  const port = (list, name) => (list ?? []).find((p) => p.name === name)?.side;
  const sourceSide = edge.source_anchor?.side ?? port(from?.outputs, edge.source.ports?.[0] ?? edge.source.port) ?? "right";
  const targetSide = edge.target_anchor?.side ?? edge.target_side ?? port(to?.inputs, edge.target.port) ?? "left";
  const ends = [
    borderPoint(from, sizeOf(edge.source.node), edge.source_anchor, sourceSide),
    ...(edge.mode === "manual" ? (edge.waypoints ?? []) : []),
    borderPoint(to, sizeOf(edge.target.node), edge.target_anchor, targetSide),
  ].map((p) => ({ x: Math.round(p.x), y: Math.round(p.y) }));
  return drawnRoute(ends, sourceSide, targetSide);
}

/** The source ports an edge carries, sorted. */
const portsOf = (edge) => [...(edge.source.ports ?? [edge.source.port])].sort();

/** Whether the canvas names the ports on an edge (#845: two outputs or more, unless turned off). */
function showsOutputLabels(edge, sourceNode) {
  return edge.show_output_labels ?? (sourceNode?.outputs ?? []).length >= 2;
}

/** Two conditions are the same clause, whatever their keys' order. */
function sameCondition(a, b) {
  return JSON.stringify(sortKeys(a ?? null)) === JSON.stringify(sortKeys(b ?? null));
}

function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((k) => [k, sortKeys(value[k])]));
  return value;
}

/**
 * Where `saved` (what the gestures saved) differs visibly from `target`, one
 * line per difference, empty when the canvas would read the same. It compares
 * what the canvas draws: the node set and positions, the edges (their ends,
 * condition, anchors, route, output and condition label positions, the ports
 * when their labels show) and the loop regions (members, bound). What the
 * canvas does not draw (prompts, harness, port names without a label, a region's
 * generated id) is left out. Nodes match by name, since the editor gives a new
 * node its own id.
 */
export function gestureDrift(saved, target, { tolerance = DRIFT_TOLERANCE, sizes = {} } = {}) {
  const drift = [];
  const byName = new Map((target.nodes ?? []).map((n) => [n.name ?? n.id, n]));
  // A saved node id → the target id of the node with its name.
  const idOf = new Map((saved.nodes ?? []).map((n) => [n.id, byName.get(n.name ?? n.id)?.id ?? n.id]));
  const measured = Object.fromEntries(Object.entries(sizes).map(([id, size]) => [idOf.get(id) ?? id, size]));
  const sizeOf = (id) => measured[id] ?? CARD;
  const savedAsTarget = { ...saved, nodes: (saved.nodes ?? []).map((n) => ({ ...n, id: idOf.get(n.id) })) };
  const as = (edge) => ({ ...edge, source: { ...edge.source, node: idOf.get(edge.source.node) ?? edge.source.node }, target: { ...edge.target, node: idOf.get(edge.target.node) ?? edge.target.node } });

  const savedNodes = new Map((saved.nodes ?? []).map((n) => [idOf.get(n.id), n]));
  for (const want of target.nodes ?? []) {
    const have = savedNodes.get(want.id);
    if (!have) {
      drift.push(`node ${want.id}: missing`);
      continue;
    }
    if (have.type !== want.type) drift.push(`node ${want.id}: type ${have.type} vs target ${want.type}`);
    const [h, w] = [have.view ?? { x: 0, y: 0 }, want.view ?? { x: 0, y: 0 }];
    if (far(h, w, tolerance)) drift.push(`node ${want.id}: at ${fmt(h)} vs target ${fmt(w)}`);
    if (isolated(have) !== isolated(want)) drift.push(`node ${want.id}: ${isolated(have) ? "isolated" : "shares the run worktree"} vs target ${isolated(want) ? "isolated" : "shares the run worktree"}`);
  }
  const targetIds = new Set((target.nodes ?? []).map((n) => n.id));
  for (const id of savedNodes.keys()) if (!targetIds.has(id)) drift.push(`node ${id}: not in the target`);

  const nodeById = new Map((target.nodes ?? []).map((n) => [n.id, n]));
  const savedEdges = new Map((saved.edges ?? []).map((e) => [edgeKey(as(e)), as(e)]));
  for (const want of target.edges ?? []) {
    const key = edgeKey(want);
    const have = savedEdges.get(key);
    if (!have) {
      drift.push(`edge ${key}: missing`);
      continue;
    }
    const at = `edge ${key}`;
    if (showsOutputLabels(want, nodeById.get(want.source.node)) && portsOf(have).join(",") !== portsOf(want).join(",")) {
      drift.push(`${at}: carries ${portsOf(have).join(", ")} vs target ${portsOf(want).join(", ")}`);
    }
    if (!sameCondition(have.when, want.when)) drift.push(`${at}: condition ${JSON.stringify(have.when ?? null)} vs target ${JSON.stringify(want.when ?? null)}`);
    if ((have.target_side ?? "left") !== (want.target_side ?? "left")) drift.push(`${at}: lands on ${have.target_side ?? "left"} vs target ${want.target_side ?? "left"}`);
    for (const key of ["source_anchor", "target_anchor"]) {
      const [h, w] = [have[key]?.side, want[key]?.side];
      if (h !== w) drift.push(`${at}: ${key} on ${h ?? "none"} vs target ${w ?? "none"}`);
    }
    // The drawn route, end to end: an anchor's offset shows here too.
    const [hr, wr] = [route(have, savedAsTarget, sizeOf), route(want, target, sizeOf)];
    if (routeDistance(hr, wr) > tolerance) drift.push(`${at}: route ${hr.map(fmt).join(" ")} vs target ${wr.map(fmt).join(" ")}`);
    if (want.when !== undefined || have.when !== undefined) {
      const [h, w] = [have.condition_label_pos, want.condition_label_pos];
      if (!h !== !w || (h && far(h, w, tolerance))) drift.push(`${at}: condition label at ${fmt(h)} vs target ${fmt(w)}`);
    }
    for (const port of new Set([...Object.keys(have.output_label_pos ?? {}), ...Object.keys(want.output_label_pos ?? {})])) {
      const [h, w] = [have.output_label_pos?.[port], want.output_label_pos?.[port]];
      if (!h !== !w || (h && far(h, w, tolerance))) drift.push(`${at}: output label ${port} at ${fmt(h)} vs target ${fmt(w)}`);
    }
  }
  const targetKeys = new Set((target.edges ?? []).map(edgeKey));
  for (const key of savedEdges.keys()) if (!targetKeys.has(key)) drift.push(`edge ${key}: not in the target`);

  const region = (loop) => [...loop.members].map((m) => idOf.get(m) ?? m).sort().join(",");
  const savedLoops = new Map((saved.loops ?? []).map((l) => [region(l), l]));
  for (const want of target.loops ?? []) {
    const have = savedLoops.get(region(want));
    if (!have) drift.push(`loop region ${region(want)}: missing`);
    else if (have.kind !== want.kind || have.max_iter !== want.max_iter) drift.push(`loop region ${region(want)}: ${have.kind} ↻ ${have.max_iter} vs target ${want.kind} ↻ ${want.max_iter}`);
  }
  const targetRegions = new Set((target.loops ?? []).map(region));
  for (const key of savedLoops.keys()) if (!targetRegions.has(key)) drift.push(`loop region ${key}: not in the target`);
  return drift;
}

/** The manifest warnings for a drift: one per difference, named as such. */
export function driftWarnings(drift) {
  return drift.map((line) => `drift from the target: ${line}`);
}
