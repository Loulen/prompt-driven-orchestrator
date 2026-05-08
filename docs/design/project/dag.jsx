// dag.jsx — DAG canvas: nodes, edges, halt icons, run overlay
// Ports are per-side configurable (left | right | top | bottom).

const NODE_W = 200, NODE_H = 70;

function statusBorder(s) {
  return ({
    running: 'var(--st-running)', done: 'var(--st-done)', blocked: 'var(--st-blocked)',
    awaiting_user: 'var(--st-await)', failed: 'var(--st-failed)', pending: 'var(--st-pending)',
  })[s] || 'var(--st-pending)';
}

// Resolve port side: explicit override OR default by kind (in=left, out=right).
function portSide(node, portName, kind) {
  if (node && node.portSides && node.portSides[portName]) return node.portSides[portName];
  return kind === 'in' ? 'left' : 'right';
}

// Anchor of a port on a node + outward direction unit vector.
function portAnchor(node, side) {
  const cx = node.x + NODE_W / 2;
  const cy = node.y + NODE_H / 2;
  switch (side) {
    case 'left':   return { x: node.x,            y: cy,             dx: -1, dy: 0 };
    case 'right':  return { x: node.x + NODE_W,   y: cy,             dx: +1, dy: 0 };
    case 'top':    return { x: cx,                y: node.y,         dx: 0,  dy: -1 };
    case 'bottom': return { x: cx,                y: node.y + NODE_H,dx: 0,  dy: +1 };
  }
  return { x: node.x + NODE_W, y: cy, dx: 1, dy: 0 };
}

// Distribute multiple handles along a side: returns inline style per index.
function handleStyleFor(side, idx, total) {
  // Evenly spread along the side (excluding very edges).
  const t = total === 1 ? 0.5 : 0.2 + (idx * 0.6) / (total - 1);
  if (side === 'left')   return { left: -7,           top: `calc(${t * 100}% - 6px)` };
  if (side === 'right')  return { right: -7,          top: `calc(${t * 100}% - 6px)` };
  if (side === 'top')    return { top: -7,            left: `calc(${t * 100}% - 6px)` };
  if (side === 'bottom') return { bottom: -7,         left: `calc(${t * 100}% - 6px)` };
  return {};
}

function TriHandle({ side = 'left', kind = 'in', active = false, style }) {
  let points;
  const inward = (kind === 'in');
  if (side === 'left')   points = inward ? "2,5 2,11 10,8" : "10,5 10,11 2,8";
  if (side === 'right')  points = inward ? "10,5 10,11 2,8" : "2,5 2,11 10,8";
  if (side === 'top')    points = inward ? "5,2 11,2 8,10" : "5,10 11,10 8,2";
  if (side === 'bottom') points = inward ? "5,10 11,10 8,2" : "5,2 11,2 8,10";
  return (
    <span className={"tri-handle side-" + side + (active ? ' active' : '')} style={style}>
      <svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points={points}/></svg>
    </span>
  );
}

function Node({ node, selected, onSelect }) {
  const { id, nid, name, type, status, x, y, iter, ports = {} } = node;
  const flowing = (status === 'running' || status === 'done');

  // Group ports by side for distribution.
  const handles = [];
  const bySide = { left: [], right: [], top: [], bottom: [] };
  (ports.in || []).forEach(p => bySide[portSide(node, p, 'in')].push({ port: p, kind: 'in' }));
  (ports.out || []).forEach(p => bySide[portSide(node, p, 'out')].push({ port: p, kind: 'out' }));
  Object.entries(bySide).forEach(([side, list]) => {
    list.forEach((h, i) => handles.push({
      ...h, side,
      style: handleStyleFor(side, i, list.length),
      active: h.kind === 'out' && flowing,
    }));
  });

  return (
    <div className={"node " + status + (selected ? " selected" : "")}
         style={{ left: x, top: y }}
         onClick={(e) => { e.stopPropagation(); onSelect && onSelect(id); }}>
      <div className="node-head">
        <span className={"st-dot " + status} />
        <span className="node-name">{name}</span>
        {iter && <span className="node-iter mono">iter {iter}</span>}
      </div>
      {nid && <div className="mono" style={{fontSize: 9, color: 'var(--fg-4)', letterSpacing: '0.02em', marginTop: -2}}>{nid}</div>}
      <div className="node-meta">
        <span className={"badge " + (type === 'code' ? 'code' : 'doc')}>
          {type === 'code' ? <Ic.Code/> : <Ic.Doc/>}
          {type === 'code' ? 'code' : 'doc'}
        </span>
        <span className="node-status mono">
          {status === 'running' ? '· active' :
           status === 'done' ? '· complete' :
           status === 'blocked' ? '· blocked' :
           status === 'awaiting_user' ? '· awaiting' :
           status === 'failed' ? '· failed' : '· pending'}
        </span>
      </div>
      {handles.map((h, i) => (
        <TriHandle key={h.kind + '-' + h.port} side={h.side} kind={h.kind} active={h.active} style={h.style}/>
      ))}
    </div>
  );
}

// Bezier from one anchor (with outward dir) to another (with outward dir).
function bezierBetween(a, b) {
  // Cap the control distance so very long edges don't blow off-canvas.
  const dist = Math.max(60, Math.min(180, Math.hypot(b.x - a.x, b.y - a.y) * 0.45));
  const c1x = a.x + a.dx * dist, c1y = a.y + a.dy * dist;
  const c2x = b.x + b.dx * dist, c2y = b.y + b.dy * dist;
  const d = `M ${a.x} ${a.y} C ${c1x} ${c1y}, ${c2x} ${c2y}, ${b.x} ${b.y}`;
  // mid: rough midpoint shifted along the perpendicular
  const mx = (a.x + b.x) / 2 + (a.dx + b.dx) * dist * 0.25;
  const my = (a.y + b.y) / 2 + (a.dy + b.dy) * dist * 0.25;
  return { d, mid: { x: mx, y: my } };
}

function Edges({ nodes, edges, runningSet }) {
  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  return (
    <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
      <defs>
        <marker id="arr" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto">
          <path d="M0 0 L8 4 L0 8 z" fill="#525a68"/>
        </marker>
        <marker id="arr-active" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto">
          <path d="M0 0 L8 4 L0 8 z" fill="#10b981"/>
        </marker>
        <marker id="arr-warn" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto">
          <path d="M0 0 L8 4 L0 8 z" fill="#f59e0b"/>
        </marker>
      </defs>
      {edges.filter(e => e.to !== 'halt').map(e => {
        const a = byId[e.from], b = byId[e.to];
        if (!a || !b) return null;
        const aSide = e.fromSide || (e.fromPort ? portSide(a, e.fromPort, 'out') : 'right');
        const bSide = e.toSide   || (e.toPort   ? portSide(b, e.toPort,   'in')  : 'left');
        const { d } = bezierBetween(portAnchor(a, aSide), portAnchor(b, bSide));
        const active = runningSet.has(e.from) && (b.status === 'running' || b.status === 'pending');
        const cond = e.cond;
        return (
          <path key={e.id} d={d}
            stroke={cond ? "#f59e0b88" : (active ? "#10b981" : "#3a414c")}
            strokeWidth="1.5"
            strokeDasharray={cond ? "4 4" : "none"}
            fill="none"
            markerEnd={cond ? "url(#arr-warn)" : (active ? "url(#arr-active)" : "url(#arr)")} />
        );
      })}
    </svg>
  );
}

function EdgeLabels({ nodes, edges }) {
  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  return (
    <>
      {edges.filter(e => e.cond && e.to !== 'halt').map(e => {
        const a = byId[e.from], b = byId[e.to];
        if (!a || !b) return null;
        const aSide = e.fromSide || (e.fromPort ? portSide(a, e.fromPort, 'out') : 'right');
        const bSide = e.toSide   || (e.toPort   ? portSide(b, e.toPort,   'in')  : 'left');
        const { mid } = bezierBetween(portAnchor(a, aSide), portAnchor(b, bSide));
        return (
          <div key={e.id} className="edge-label cond" style={{ left: mid.x - 60, top: mid.y - 12 }}>
            when: {e.cond}
          </div>
        );
      })}
    </>
  );
}

function HaltIcons({ nodes, edges }) {
  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  return (
    <>
      {edges.filter(e => e.to === 'halt').map(e => {
        const a = byId[e.from];
        if (!a) return null;
        const aSide = e.fromPort ? portSide(a, e.fromPort, 'out') : 'right';
        const aA = portAnchor(a, aSide);
        const hx = aA.x + aA.dx * 80 + 20;
        const hy = aA.y + aA.dy * 80 + (aA.dx === 0 ? 0 : 80);
        const c1x = aA.x + aA.dx * 40, c1y = aA.y + aA.dy * 40;
        const d = `M ${aA.x} ${aA.y} C ${c1x} ${c1y}, ${hx - 20} ${hy}, ${hx} ${hy}`;
        return (
          <React.Fragment key={e.id}>
            <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
              <path d={d} stroke="#f97316" strokeWidth="1.5" strokeDasharray="4 4" fill="none"/>
            </svg>
            <div className="halt-icon" style={{ left: hx - 14, top: hy - 14 }}>◌</div>
            <div className="edge-label cond" style={{ left: hx - 70, top: hy - 32, color: '#fdba74', borderColor: 'rgba(249,115,22,0.32)', background: 'rgba(249,115,22,0.10)' }}>
              halt: {e.cond}
            </div>
          </React.Fragment>
        );
      })}
    </>
  );
}

function MiniMap({ nodes }) {
  const W = 180, H = 110;
  const xs = nodes.map(n => n.x), ys = nodes.map(n => n.y);
  const minX = Math.min(...xs) - 40, maxX = Math.max(...xs) + 240;
  const minY = Math.min(...ys) - 40, maxY = Math.max(...ys) + 110;
  const sx = (W - 12) / (maxX - minX), sy = (H - 12) / (maxY - minY);
  const s = Math.min(sx, sy);
  return (
    <div className="minimap">
      <svg width={W} height={H}>
        {nodes.map(n => (
          <rect key={n.id}
            x={6 + (n.x - minX) * s}
            y={6 + (n.y - minY) * s}
            width={NODE_W * s}
            height={NODE_H * s}
            rx="2"
            fill={statusBorder(n.status)}
            opacity={n.status === 'pending' ? 0.4 : 0.85}/>
        ))}
        <rect x="4" y="4" width={W - 8} height={H - 8} rx="3" fill="none" stroke="rgba(255,255,255,0.12)"/>
      </svg>
    </div>
  );
}

function CanvasControls() {
  return (
    <div className="canvas-controls">
      <button title="Zoom in"><Ic.Plus/></button>
      <button title="Zoom out"><Ic.Minus/></button>
      <button title="Fit"><Ic.Maximize/></button>
    </div>
  );
}

function RunOverlay({ run, blocked = false, onOpenManager, editingRun = false, onToggleEditRun }) {
  const terminal = run.status === 'done' || run.status === 'failed' || run.status === 'archived';
  return (
    <div className="run-overlay">
      <div className="ro-head">
        <span className={"st-dot " + run.status} style={editingRun ? {animation: 'none'} : {}}/>
        <span className="ro-title">{run.pipeline}</span>
        <span className={"badge " + (run.status === 'running' ? 'running' : run.status === 'blocked' ? 'blocked' : run.status === 'awaiting_user' ? 'awaiting' : 'done')}>
          {run.status === 'awaiting_user' ? 'awaiting' : run.status}
        </span>
      </div>
      <div className="ro-row"><span className="ro-label">run-id</span><span className="ro-id">{run.id.slice(-12)} <Ic.Copy className="copy-icon"/></span></div>
      <div className="ro-row"><span className="ro-label">version</span><span className="ro-value mono">v3</span></div>
      <div className="ro-row"><span className="ro-label">started</span><span className="ro-value mono">{run.when}</span></div>
      <div className="ro-row"><span className="ro-label">elapsed</span><span className="ro-value mono" style={run.status === 'running' ? {color: 'var(--st-running)'} : {}}>{run.elapsed}</span></div>
      {run.iter && <div className="ro-row"><span className="ro-label">iter</span><span className="ro-value mono">{run.iter}/5</span></div>}
      <div className="ro-row"><span className="ro-label">vars</span><span className="ro-value mono" style={{color: 'var(--fg-3)'}}>3 set →</span></div>

      {blocked && (
        <div className="halt-callout">
          <div className="hc-title"><Ic.Halt/> halted</div>
          Max iterations reached without PASS verdict. Open the manager to extend the cycle or mark the run done.
        </div>
      )}

      <div className="ro-actions-col" style={{marginTop: 12}}>
        <button className={"btn" + (blocked ? " highlight" : "")} onClick={onOpenManager}>
          <Ic.Manager/> Open Manager
        </button>
        <button className="btn"
          onClick={onToggleEditRun}
          style={editingRun ? {color: 'var(--edit-tint)', borderColor: 'rgba(167,139,250,0.32)', background: 'rgba(167,139,250,0.10)'} : {}}>
          <Ic.Pencil/> {editingRun ? 'Stop editing' : 'Edit this run'}
        </button>
        {run.status === 'running' && !editingRun && <button className="btn warn"><Ic.X/> Cancel</button>}
        {terminal && <button className="btn">Cleanup</button>}
      </div>

      {editingRun && (
        <div className="ro-edit-foot mono">
          Editing run-scoped copy · template unchanged
        </div>
      )}
    </div>
  );
}

window.Node = Node;
window.Edges = Edges;
window.EdgeLabels = EdgeLabels;
window.HaltIcons = HaltIcons;
window.MiniMap = MiniMap;
window.CanvasControls = CanvasControls;
window.RunOverlay = RunOverlay;
window.portSide = portSide;
window.portAnchor = portAnchor;
window.NODE_W = NODE_W;
window.NODE_H = NODE_H;
