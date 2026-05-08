// loop-node.jsx — Loop node + inspector

function LoopNode({ node, selected, onSelect, runMode, currentIter }) {
  const max = node.maxIter || 5;
  return (
    <div className={"node loop-node" + (selected ? " selected" : "")}
    style={{ left: node.x, top: node.y, width: 200 }}
    onClick={(e) => {e.stopPropagation();onSelect && onSelect(node.id);}}>
      <div className="node-head">
        <span className="lp-icon">
          <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
            <path d="M11 5.5a4.5 4.5 0 1 0-1.3 3.2" />
            <path d="M11 2.5v3h-3" />
          </svg>
        </span>
        <span className="node-name">Loop</span>
        {node.nid && <span className="node-iter mono">{node.nid}</span>}
      </div>
      <div className="lp-body">
        <span className="lp-badge mono">↻ {runMode ? `${currentIter || 0}/${max}` : `max ${max}`}</span>
      </div>
      <div className="lp-ports">
        <span className="lp-port-in mono">in</span>
        <span className="lp-port-out mono">body</span>
      </div>
      <div className="lp-ports lp-ports-2">
        <span className="lp-port-in mono">done</span>
        <span className="lp-port-out mono">break</span>
      </div>
      <span className="tri-handle side-left"><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8" /></svg></span>
      <span className="tri-handle side-left lp-handle-2"><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8" /></svg></span>
      <span className="tri-handle side-right"><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8" /></svg></span>
      <span className="tri-handle side-right lp-handle-2"><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8" /></svg></span>
    </div>);

}

function LoopInspector({ node }) {
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
          <input className="input" defaultValue={node.name || 'Loop'} />
        </div>
      </div>
      <div className="p-sect">
        <SectionHead title="Configuration" />
        <div className="field">
          <label>max_iter <span className="info-mark" data-tip="Maximum iterations of the loop body before firing `done`. Can also reference a pipeline variable like $max_iter_review.">ⓘ</span></label>
          <input className="input mono" defaultValue={node.maxIter || 5} style={{ width: 120 }} />
        </div>
      </div>
      <div className="p-sect">
        <SectionHead title="Ports" count={4}/>
        <div style={{display:'flex', flexDirection:'column', gap: 8}}>
          {[
            {name:'in',    dir:'in',  side:'left'},
            {name:'break', dir:'in',  side:'left'},
            {name:'body',  dir:'out', side:'right'},
            {name:'done',  dir:'out', side:'right'},
          ].map(p => (
            <div key={p.name} className="port-row" style={{gridTemplateColumns: '12px 1fr auto', alignItems: 'center'}}>
              <span className={"pdot" + (p.dir === 'out' ? ' out' : '')}/>
              <div>
                <span className="mono" style={{fontSize: 11.5, color: 'var(--fg-2)'}}>{p.name}</span>
                <span className="badge" style={{marginLeft: 6, fontSize: 9.5}}>{p.dir === 'in' ? 'input' : 'output'}</span>
              </div>
              <SidePicker value={p.side}/>
            </div>
          ))}
        </div>
        <div className="help" style={{marginTop: 10, lineHeight: 1.5}}>
          Port names are fixed: <span className="mono">in</span>, <span className="mono">break</span> (inputs); <span className="mono">body</span>, <span className="mono">done</span> (outputs). Side placement is configurable per port.
        </div>
      </div>
    </div>);

}

window.LoopNode = LoopNode;
window.LoopInspector = LoopInspector;