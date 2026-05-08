// switch-node.jsx — Switch node + inspector

function SwitchNode({ node, selected, onSelect, activeBranch }) {
  const branches = node.branches || [];
  return (
    <div className={"node switch-node" + (selected ? " selected" : "")}
    style={{ left: node.x, top: node.y, width: 220 }}
    onClick={(e) => {e.stopPropagation();onSelect && onSelect(node.id);}}>
      <div className="node-head">
        <span className="sw-icon">
          <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
            <path d="M2 6.5h3l2-3h4M9 3.5l2-1M9 3.5l1.5 2M2 6.5l3 3h4M9 9.5l2 1M9 9.5l1.5-2" />
          </svg>
        </span>
        <span className="node-name">Switch</span>
        {node.nid && <span className="node-iter mono">{node.nid}</span>}
      </div>
      <div className="sw-branches">
        {branches.map((b, i) =>
        <div key={i} className={"sw-branch" + (b.isDefault ? ' is-default' : '') + (activeBranch === b.name ? ' active' : '') + (activeBranch && activeBranch !== b.name ? ' dim' : '')}>
            <span className="sw-bname mono">{b.name}</span>
            {b.isDefault && <span className="sw-bcond mono">
</span>}
            <span className="sw-bport" />
          </div>)}
      </div>
      <span className="tri-handle side-left"><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8" /></svg></span>
    </div>);

}

function SwitchInspector({ node, focusedRow }) {
  const branches = node.branches || [];
  return (
    <div className="p-body">
      <div className="p-sect">
        <SectionHead title="Identity" />
        <div className="field">
          <label>id <span style={{ color: 'var(--fg-4)', fontWeight: 400 }}>· immutable</span></label>
          <div className="input mono" style={{ background: 'var(--bg-0)', color: 'var(--fg-3)' }}>{node.nid}</div>
        </div>
        <div className="field">
          <label>Display name</label>
          <input className="input" defaultValue={node.name || 'Switch'} />
        </div>
      </div>

      <div className="p-sect">
        <SectionHead title="Branches" count={branches.length} />
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {branches.map((b, i) =>
          <div key={i} className={"branch-card" + (b.isDefault ? ' is-default' : '')}>
              <div className="branch-head">
                <span className="grip mono" title="Drag to reorder">⋮⋮</span>
                {b.isDefault ?
              <>
                    <span className="branch-name mono">{b.name}</span>
                    <span className="badge" style={{ marginLeft: 6 }}>else</span>
                  </> :

              <input className="input mono" defaultValue={b.name} style={{ flex: 1, height: 24, fontSize: 11 }} />
              }
                {!b.isDefault && <button className="icon-btn" style={{ width: 22, height: 22 }}><Ic.Trash /></button>}
              </div>
              {!b.isDefault &&
            <>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginTop: 6 }}>
                    {(b.conditions || []).map((c, j) =>
                <div key={j} className={"cond-row" + (focusedRow && focusedRow.branch === i && focusedRow.cond === j ? ' focused' : '')}>
                        <input className="input mono" defaultValue={c.field} style={{ height: 22, fontSize: 10.5 }} />
                        <select className="select mono" defaultValue={c.op} style={{ height: 22, fontSize: 10.5, padding: '0 4px' }}>
                          <option value="eq">eq</option>
                          <option value="neq">neq</option>
                          <option value="lt">lt</option>
                          <option value="lte">lte</option>
                          <option value="gt">gt</option>
                          <option value="gte">gte</option>
                          <option value="in">in</option>
                          <option value="not_in">not_in</option>
                        </select>
                        <input className={"input mono" + (focusedRow && focusedRow.branch === i && focusedRow.cond === j ? ' focused' : '')}
                  defaultValue={c.value} style={{ height: 22, fontSize: 10.5 }} />
                        <button className="icon-btn" style={{ width: 22, height: 22 }}><Ic.X /></button>
                      </div>
                )}
                  </div>
                  <button className="btn ghost sm" style={{ marginTop: 6 }}><Ic.PlusSm /> Add condition</button>
                </>
            }
            </div>
          )}
        </div>
        <div className="help" style={{ marginTop: 10, lineHeight: 1.5 }}>
          All conditions in a branch are AND'd. For OR, use <span className="mono" style={{ color: 'var(--fg-3)' }}>in [...]</span> or add another branch with the same target.
        </div>
      </div>

      <div className="p-sect">
        <SectionHead title="Ports" count={branches.length + 1}/>
        <div style={{display:'flex', flexDirection:'column', gap: 8}}>
          <div className="port-row" style={{gridTemplateColumns: '12px 1fr auto', alignItems: 'center'}}>
            <span className="pdot"/>
            <div>
              <span className="mono" style={{fontSize: 11.5, color: 'var(--fg-2)'}}>in</span>
              <span className="badge" style={{marginLeft: 6, fontSize: 9.5}}>input</span>
            </div>
            <SidePicker value="left"/>
          </div>
          {branches.map((b, i) => (
            <div key={i} className="port-row" style={{gridTemplateColumns: '12px 1fr auto', alignItems: 'center'}}>
              <span className="pdot out"/>
              <div>
                <span className="mono" style={{fontSize: 11.5, color: 'var(--fg-2)'}}>{b.name}</span>
                <span className="badge" style={{marginLeft: 6, fontSize: 9.5}}>output</span>
              </div>
              <SidePicker value="right"/>
            </div>
          ))}
        </div>
        <div className="help" style={{marginTop: 10, lineHeight: 1.5}}>
          Port names follow branch names. Side placement is configurable per port.
        </div>
      </div>
    </div>);
}

window.SwitchNode = SwitchNode;
window.SwitchInspector = SwitchInspector;