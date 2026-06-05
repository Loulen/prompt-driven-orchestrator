// refonte-canvas.jsx — pipeline-editor refonte canvas primitives
// Slim node cards · per-document output dots · dot-less emergent inputs ·
// always-visible condition pills · orthogonal user-anchored edges ·
// translucent loop regions. Dumb renderers; screens author geometry.

const RF_W = 168, RF_H = 46;

function rfBox(n) { return { x: n.x, y: n.y, w: n.w || RF_W, h: n.h || RF_H }; }

// Point on a node side. t in 0..1 along the side. Returns outward unit dir too.
function sidePoint(box, side, t = 0.5) {
  switch (side) {
    case 'left':   return { x: box.x,          y: box.y + box.h * t, dx: -1, dy: 0 };
    case 'right':  return { x: box.x + box.w,   y: box.y + box.h * t, dx: 1,  dy: 0 };
    case 'top':    return { x: box.x + box.w*t, y: box.y,             dx: 0,  dy: -1 };
    case 'bottom': return { x: box.x + box.w*t, y: box.y + box.h,     dx: 0,  dy: 1 };
  }
  return { x: box.x + box.w, y: box.y + box.h/2, dx: 1, dy: 0 };
}

// Absolute anchor of a named output dot on a node.
function outAnchor(n, name) {
  const o = (n.outs || []).find(o => o.name === name) || {};
  return sidePoint(rfBox(n), o.side || 'right', o.t == null ? 0.5 : o.t);
}

// Rounded-corner orthogonal path through an explicit list of points.
function orthPath(points, r = 9) {
  if (!points || points.length < 2) return '';
  const norm = (p, q) => { const dx = q.x - p.x, dy = q.y - p.y, m = Math.hypot(dx, dy) || 1; return { x: dx/m, y: dy/m }; };
  let d = `M ${points[0].x} ${points[0].y}`;
  for (let i = 1; i < points.length - 1; i++) {
    const p = points[i], prev = points[i-1], next = points[i+1];
    const rr = Math.min(r, Math.hypot(p.x-prev.x, p.y-prev.y)/2, Math.hypot(next.x-p.x, next.y-p.y)/2);
    const v1 = norm(p, prev), v2 = norm(p, next);
    const a = { x: p.x + v1.x*rr, y: p.y + v1.y*rr };
    const b = { x: p.x + v2.x*rr, y: p.y + v2.y*rr };
    d += ` L ${a.x.toFixed(1)} ${a.y.toFixed(1)} Q ${p.x} ${p.y} ${b.x.toFixed(1)} ${b.y.toFixed(1)}`;
  }
  const last = points[points.length - 1];
  d += ` L ${last.x} ${last.y}`;
  return d;
}

// Generic agent role glyph (chip-with-pins). Kept simpler than a square.
function RfRole({ role = 'agent' }) {
  if (role === 'merge') return (
    <svg width="15" height="15" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 2.6 v3.2 c0 2.4 1.8 3 4 3.6 M3 11.4 v-3.2 c0-2.4 1.8-3 4-3.6 M8.6 5 l2.4 -.6 M8.6 9 l2.4 .6"/>
    </svg>
  );
  return (
    <svg width="15" height="15" viewBox="0 0 14 14" fill="none">
      <path d="M7 2.3 L10.3 7 L7 11.7 L3.7 7 Z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round"/>
      <circle cx="7" cy="7" r="1.05" fill="currentColor"/>
    </svg>
  );
}

// ── Slim node card ────────────────────────────────────────────────────────
function RfNode({ node, selected, onSelect }) {
  const { id, name, sub, role = 'agent', mark = 'code', status = 'pending', outs = [], badge } = node;
  const box = rfBox(node);
  const running = status === 'running';
  const cls = ['rf-node', status === 'fired' ? 'fired' : status, selected ? 'selected' : ''].join(' ');
  return (
    <>
      <div className={cls} style={{ left: box.x, top: box.y, width: box.w, minHeight: box.h }}
           onClick={(e) => { e.stopPropagation(); onSelect && onSelect(id); }}>
        <span className="rf-ntype" style={{ position: 'relative' }}>
          <RfRole role={role}/>
          {running && <span className="rf-run-pulse"/>}
        </span>
        <span className="rf-nbody">
          <span className="rf-nname">{name}</span>
          {sub && <span className="rf-nsub">{sub}</span>}
        </span>
        <span className={"rf-mark " + mark} title={mark === 'code' ? 'code-mutating' : 'doc-only'}>
          {mark === 'code' ? <Ic.Code/> : <Ic.Doc/>}
        </span>
        {badge && <span className={"rf-badge " + badge.flavor}><span style={{fontFamily:'var(--font-mono)'}}>{badge.glyph}</span>{badge.text}</span>}
      </div>
      {/* output dots — one per produced document */}
      {outs.map((o, i) => {
        const a = sidePoint(box, o.side || 'right', o.t == null ? 0.5 : o.t);
        return (
          <span key={o.name} className={"rf-out" + (o.produced ? ' produced' : '') + (o.empty ? ' empty' : '')}
                style={{ left: a.x - 5.5, top: a.y - 5.5 }} title={o.name}>
            {o.showLabel && <span className={"rf-out-lbl shown"}>{o.name}</span>}
          </span>
        );
      })}
    </>
  );
}

// ── Edge layer (orthogonal) ─────────────────────────────────────────────────
function RfEdges({ edges }) {
  return (
    <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none', zIndex: 2 }}>
      <defs>
        <marker id="rf-arr" viewBox="0 0 8 8" refX="6.5" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
          <path d="M0 0 L8 4 L0 8 z" fill="#566070"/>
        </marker>
        <marker id="rf-arr-acc" viewBox="0 0 8 8" refX="6.5" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
          <path d="M0 0 L8 4 L0 8 z" fill="#c2a15e"/>
        </marker>
        <marker id="rf-arr-sel" viewBox="0 0 8 8" refX="6.5" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto-start-reverse">
          <path d="M0 0 L8 4 L0 8 z" fill="#c2a15e"/>
        </marker>
      </defs>
      {edges.map(e => {
        const acc = e.accent;
        const sel = e.selected;
        return (
          <path key={e.id} d={orthPath(e.points)}
            stroke={sel ? '#c2a15e' : (acc ? '#c2a15e' : '#3f4754')}
            strokeWidth={sel ? 2 : 1.6}
            strokeDasharray={e.isElse ? '5 5' : 'none'}
            fill="none"
            markerEnd={sel ? 'url(#rf-arr-sel)' : (acc ? 'url(#rf-arr-acc)' : 'url(#rf-arr)')}
            opacity={e.dim ? 0.4 : 1}/>
        );
      })}
    </svg>
  );
}

// ── Condition pills (always visible at edge midpoint) ───────────────────────
function CondPill({ x, y, when, isElse, selected, onSelect }) {
  if (isElse) return (
    <div className={"rf-pill else" + (selected ? ' selected' : '')} style={{ left: x, top: y }}
         onClick={(e) => { e.stopPropagation(); onSelect && onSelect(); }}>
      <span className="pk">else</span>
    </div>
  );
  const w = when || {};
  return (
    <div className={"rf-pill" + (selected ? ' selected' : '')} style={{ left: x, top: y }}
         onClick={(e) => { e.stopPropagation(); onSelect && onSelect(); }}>
      <span className={w.field === 'iter' ? 'iter' : 'pk'}>{w.field}</span>
      <span className="pop">{w.op}</span>
      <span className="pv">{String(w.val)}</span>
    </div>
  );
}

// ── Translucent loop region (grouping layer behind nodes) ───────────────────
function LoopRegion({ box, flavor = 'bounded', name, counter, items, exhausted, blockText, onSelect }) {
  return (
    <div className={"rf-region " + (exhausted ? 'exhausted ' : '') + flavor}
         style={{ left: box.x, top: box.y, width: box.w, height: box.h }}>
      <div className="rf-region-head" onClick={(e) => { e.stopPropagation(); onSelect && onSelect(); }}>
        {flavor === 'bounded'
          ? <><span className="rh-glyph">↻</span><span className="rh-count">{counter}</span></>
          : <><span className="rh-glyph">⇉</span><span className="rh-count">{items} items</span></>}
        {name && <span className="rh-name">{name}</span>}
      </div>
      {exhausted && blockText && (
        <div className="rf-region-block"><Ic.Halt style={{ width: 11, height: 11 }}/>{blockText}</div>
      )}
    </div>
  );
}

// ── Emergent input hover-name labels (shown statically as examples) ─────────
function InputLabels({ labels = [] }) {
  return (
    <>
      {labels.map((l, i) => (
        <div key={i} className="rf-in-lbl" style={{ left: l.x, top: l.y }}>
          {l.name}{l.from && <><span className="arr">←</span>{l.from}</>}
        </div>
      ))}
    </>
  );
}

// ── Refonte Start node — shows submitted prompt + input images ──────────────
function RfStart({ x, y, when = '4 min ago', runIdSlug, images = [], selected, onSelect }) {
  return (
    <div className={"start-node" + (selected ? " selected" : "")}
         style={{ left: x, top: y, height: 'auto', minHeight: 56, paddingTop: 10, paddingBottom: images.length ? 10 : 0, alignItems: 'flex-start', borderRadius: 16, flexDirection: 'column', width: 200 }}
         onClick={(e) => { e.stopPropagation(); onSelect && onSelect('__start'); }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, width: '100%' }}>
        <span className="sn-play">
          <svg width="10" height="11" viewBox="0 0 10 11" fill="currentColor"><path d="M1 1.2v8.6a.5.5 0 0 0 .77.42l7.2-4.3a.5.5 0 0 0 0-.84L1.77.78A.5.5 0 0 0 1 1.2z"/></svg>
        </span>
        <span className="sn-main">
          <span className="sn-label">Run start</span>
          <span className="sn-meta">started {when} · {runIdSlug}</span>
        </span>
      </div>
      {images.length > 0 && (
        <div className="rf-start-imgs">
          {images.map((im, i) => (
            <div key={i} className="rf-img-chip"><div className="ic-stripe"/><div className="ic-tag">{im}</div></div>
          ))}
        </div>
      )}
      <span className="node-handle out active" style={{ left: '100%', top: 28 }}/>
    </div>
  );
}

// ── Canvas chrome wrapper ───────────────────────────────────────────────────
function RfCanvas({ children, hint, worldTransform = 'translate(0,0)', miniNodes, overlay }) {
  const mini = (miniNodes && miniNodes.length) ? miniNodes.map((n, i) => ({ ...n, id: n.id || ('m' + i) })) : null;
  return (
    <div className="rf-canvas" onClick={(e) => e.stopPropagation()}>
      <div className="rf-world" style={{ transform: worldTransform }}>
        {children}
      </div>
      {overlay}
      {hint && (
        <div className="rf-hint">
          {hint}
        </div>
      )}
      <CanvasToolbar activeTool="select"/>
      {mini && <MiniMap nodes={mini}/>}
      <CanvasControls/>
    </div>
  );
}

Object.assign(window, {
  RF_W, RF_H, rfBox, sidePoint, outAnchor, orthPath,
  RfNode, RfEdges, CondPill, LoopRegion, InputLabels, RfStart, RfCanvas, RfRole,
});
