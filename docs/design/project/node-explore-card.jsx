// node-explore-card.jsx — NodeCard component, hypothesis-agnostic.
// The visual treatment is applied by a wrapper class (.h1..h5) higher up.

const NODE_KIND_ICON = {
  code:   () => <Ic.Code/>,
  doc:    () => <Ic.Doc/>,
  switch: () => <Ic.Switch/>,
  loop:   () => <Ic.Loop/>,
  foreach:() => <Ic.Loop/>,
  merge:  () => <Ic.Branch/>,
  start:  () => <Ic.Play/>,
  end:    () => <Ic.Check/>,
};

function NodeCard({
  status = 'pending',
  selected = false,
  kind = 'code',
  name = 'rewrite_section',
  nid = 'nd_4f2a',
  ports = { in: ['in'], out: ['out'] },
  portSides = {},          // map portName -> 'left'|'right'|'top'|'bottom'
  portStates = {},         // portName -> { drop?: bool, indicator?: 'ok'|'pending' }
  compact = false,
  debug = false,           // expose tolerance halos
  hypothesisPort = 'p1',   // port hypothesis class for the inner ports
  showFailOverlay = true,  // honor the "dominant overlay" rule when failed
  failOverlay = 'badge',   // 'badge' | 'badge+tint' | 'badge+stripe'
  width,                   // optional override
  children,
}) {
  const IconCmp = NODE_KIND_ICON[kind] || NODE_KIND_ICON.code;

  // Group ports by side first, then place them with idx/total so multiple
  // ports on the same side are distributed along it (matches dag.jsx).
  const bySide = { left: [], right: [], top: [], bottom: [] };
  (ports.in  || []).forEach(p => {
    const side = portSides[p] || 'left';
    bySide[side].push({ port: p, dir: 'in' });
  });
  (ports.out || []).forEach(p => {
    const side = portSides[p] || 'right';
    bySide[side].push({ port: p, dir: 'out' });
  });
  const portEls = [];
  Object.entries(bySide).forEach(([side, list]) => {
    list.forEach((h, i) => {
      const st = portStates[h.port] || {};
      portEls.push(
        <Port key={h.dir + '-' + h.port}
              side={side} dir={h.dir} label={h.port}
              isDrop={!!st.drop} indicator={st.indicator}
              debug={debug}
              idx={i} total={list.length}/>
      );
    });
  });

  return (
    <div className={"nc-wrap " + hypothesisPort}
         style={width ? { width } : null}>
      <div className={"nc " + (compact ? 'compact ' : '') + (selected ? 'is-selected ' : '')}
           data-status={status}>

        {/* dominant failure overlay (only on failed) */}
        {status === 'failed' && showFailOverlay && (failOverlay.includes('tint')) && (
          <div className="nc-fail-tint" aria-hidden="true"/>
        )}

        <div className="nc-head">
          <span className="nc-icon"><IconCmp/></span>
          <span className="nc-name">{name}</span>
        </div>
        {!compact && nid && <div className="nc-nid mono">{nid}</div>}
        {!compact && (
          <div className="nc-meta">
            <span className="nc-status-line">
              {status === 'pending'       && '· pending'}
              {status === 'running'       && '· active'}
              {status === 'awaiting_user' && '· awaiting'}
              {status === 'completed'     && '· complete'}
              {status === 'failed'        && '· failed'}
            </span>
          </div>
        )}

        {portEls}
        {children}

        {status === 'failed' && showFailOverlay && (
          <span className="nc-fail-badge" aria-label="failed">
            <Ic.X/>
          </span>
        )}
      </div>
    </div>
  );
}

window.NodeCard = NodeCard;
