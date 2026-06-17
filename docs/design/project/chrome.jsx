// chrome.jsx — top bar, status bar, panel headers, daemon banner

function TopBar({ mode, onToggleMode, breadcrumb, runId }) {
  return (
    <div className="top-bar">
      <div className="brand">
        <span className="brand-mark"><Ic.Logo/></span>
        PDO
      </div>
      <div className="breadcrumb">
        <span className={"b-mode" + (mode === 'edit' ? " edit" : "")}>{mode === 'edit' ? 'Edit' : 'Run'}</span>
        <span className="sep">/</span>
        <span className="mono" style={{color: 'var(--fg-2)'}}>{breadcrumb}</span>
        {runId && (<>
          <span className="sep">/</span>
          <span className="mono b-cur">{runId}</span>
        </>)}
      </div>
      <div className="top-actions">
        <button className={"icon-btn" + (mode === 'edit' ? " active" : "")}
          onClick={onToggleMode} title={mode === 'edit' ? 'Exit Edit mode' : 'Enter Edit mode'}>
          <Ic.Pencil/>
        </button>
        <button className="icon-btn" title="Settings"><Ic.Gear/></button>
        <button className="icon-btn" title="Theme"><Ic.Moon/></button>
      </div>
    </div>
  );
}

function StatusBar({ daemon = 'connected', activeRuns = 3, awaiting = 1 }) {
  return (
    <div className="status-bar">
      <div className="item">
        <span className={"dot" + (daemon === 'reconnecting' ? ' warn' : daemon === 'down' ? ' err' : '')}/>
        <span>daemon · {daemon}</span>
      </div>
      <div className="item"><span style={{color: 'var(--fg-4)'}}>·</span></div>
      <div className="item">{activeRuns} runs active</div>
      {awaiting > 0 && (
        <div className="item" style={{color: 'var(--st-await)'}}>{awaiting} awaiting user</div>
      )}
      <div className="spacer"/>
      <div className="item">v0.4.2-dev</div>
    </div>
  );
}

function PanelHead({ title, count, actions, children }) {
  return (
    <div className="p-head">
      <h3>{title}{count != null && <span style={{color: 'var(--fg-4)', fontWeight: 400, marginLeft: 6}}>{count}</span>}</h3>
      {children}
      <div className="p-actions">{actions}</div>
    </div>
  );
}

function SectionHead({ title, count, collapsed = false, onToggle }) {
  return (
    <div className={"p-sect-head" + (collapsed ? " collapsed" : "")} onClick={onToggle}>
      <span className="chev"><Ic.Chevron/></span>
      <span>{title}</span>
      {count != null && <span className="count">· {count}</span>}
    </div>
  );
}

window.TopBar = TopBar;
window.StatusBar = StatusBar;
window.PanelHead = PanelHead;
window.SectionHead = SectionHead;
