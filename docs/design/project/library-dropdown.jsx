// library-dropdown.jsx — dropdown anchored under Library toolbar icon

const LIBRARY_ENTRIES = [
  { name: 'implementer',     type: 'code', desc: 'You are an implementer. Read the plan and any prior review feedback…' },
  { name: 'reviewer',        type: 'doc',  desc: 'You are a reviewer. Inspect the diff and produce a verdict (PASS/FAIL)…' },
  { name: 'planner',         type: 'doc',  desc: 'You are a planner. Read the issue and produce a structured plan with…' },
  { name: 'merge-resolver',  type: 'code', desc: 'You resolve git merge conflicts. Read both sides, produce a coherent…' },
];

function libIcon(t) {
  if (t === 'code')   return <Ic.Code/>;
  if (t === 'doc')    return <Ic.Doc/>;
  if (t === 'loop')   return (<svg width="12" height="12" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M11 5.5a4.5 4.5 0 1 0-1.3 3.2"/><path d="M11 2.5v3h-3"/></svg>);
  if (t === 'switch') return (<svg width="12" height="12" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M2 6.5h3l2-3h4M9 3.5l2-1M9 3.5l1.5 2M2 6.5l3 3h4M9 9.5l2 1M9 9.5l1.5-2"/></svg>);
}

function LibraryDropdown({ open, onClose, hoverIndex = -1 }) {
  if (!open) return null;
  return (
    <div className="lib-dropdown" onClick={(e)=>e.stopPropagation()}>
      <div className="lib-head">
        <span className="lib-title">Library</span>
        <span className="lib-count mono">{LIBRARY_ENTRIES.length} entries</span>
      </div>
      <div className="lib-search">
        <Ic.Search/>
        <input placeholder="Filter nodes…"/>
      </div>
      <div className="lib-list">
        {LIBRARY_ENTRIES.map((e, i) => (
          <div key={e.name} className={"lib-row" + (i === hoverIndex ? ' hover' : '')}>
            <span className="lib-icon">{libIcon(e.type)}</span>
            <div className="lib-main">
              <div className="lib-name">{e.name}</div>
              <div className="lib-desc">{e.desc}</div>
            </div>
            <div className="lib-actions">
              <button className="btn ghost sm" style={{height:22, padding:'0 6px'}}><Ic.PlusSm/> Add</button>
              <button className="icon-btn" style={{width:22, height:22}}><Ic.Trash/></button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

window.LIBRARY_ENTRIES = LIBRARY_ENTRIES;
window.LibraryDropdown = LibraryDropdown;
