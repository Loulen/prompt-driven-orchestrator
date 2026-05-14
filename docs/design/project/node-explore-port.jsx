// node-explore-port.jsx — Port component, hypothesis-agnostic geometry.

function Chevron({ dir, side }) {
  // outward chevron for 'out', inward chevron for 'in'.
  // The chevron points along the side axis.
  const inward = (dir === 'in');
  let pts;
  if (side === 'left')   pts = inward ? "2,2 6,5 2,8" : "6,2 2,5 6,8";
  if (side === 'right')  pts = inward ? "6,2 2,5 6,8" : "2,2 6,5 2,8";
  if (side === 'top')    pts = inward ? "2,2 5,6 8,2" : "2,6 5,2 8,6";
  if (side === 'bottom') pts = inward ? "2,6 5,2 8,6" : "2,2 5,6 8,2";
  return (
    <svg width="8" height="8" viewBox="0 0 8 10" aria-hidden="true">
      <polyline points={pts} stroke="currentColor" strokeWidth="1.4"
        fill="none" strokeLinecap="round" strokeLinejoin="round"/>
    </svg>
  );
}

function PerPortIndicator({ state = 'pending' }) {
  // Tiny dot used on End-node inputs (per-port status, designer call).
  const cls = state === 'ok' ? 'ok' : state === 'pending' ? 'pending' : '';
  return <span className={"per-port " + cls}/>;
}

function Port({ side = 'right', dir = 'out', label = 'out',
                isDrop = false, indicator, debug = false,
                idx = 0, total = 1 }) {
  // Spread multiple ports along the same side: t in (0,1) along the axis.
  const t = total <= 1 ? 0.5 : 0.2 + (idx * 0.6) / (total - 1);
  const styleOverride = {};
  if (side === 'left' || side === 'right') {
    if (total > 1) styleOverride.top = `${t * 100}%`;
  } else {
    if (total > 1) styleOverride.left = `${t * 100}%`;
  }
  return (
    <span className={"port side-" + side
                     + (isDrop ? ' is-drop-target' : '')
                     + (debug ? ' debug' : '')}
          style={styleOverride}>
      <span className="halo"/>
      <span className="chev">
        <Chevron dir={dir} side={side}/>
      </span>
      <span className="port-label">{label}</span>
      {indicator && <PerPortIndicator state={indicator}/>}
    </span>
  );
}

window.Port = Port;
window.PerPortIndicator = PerPortIndicator;
