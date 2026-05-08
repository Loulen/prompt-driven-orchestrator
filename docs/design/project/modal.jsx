// modal.jsx — New Run modal + tabs + add-palette + empty states

function NewRunModal({ open, onClose }) {
  const [varsOpen, setVarsOpen] = React.useState(true);
  if (!open) return null;
  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>Launch new run</h2>
          <button className="icon-btn" onClick={onClose}><Ic.X/></button>
        </div>
        <div className="modal-body">
          <div className="field">
            <label>Pipeline</label>
            <select className="select mono" defaultValue="feature-with-review">
              <optgroup label="Repo pipelines">
                <option value="feature-with-review">feature-with-review</option>
                <option value="bug-triage">bug-triage</option>
              </optgroup>
              <optgroup label="User pipelines">
                <option value="doc-refresh">doc-refresh</option>
                <option value="security-audit">security-audit</option>
                <option value="release-notes">release-notes</option>
              </optgroup>
            </select>
          </div>
          <div className="field">
            <label>Input</label>
            <textarea className="textarea mono" rows="6"
              defaultValue={`Implement search filter for archived projects.\n\nhttps://github.com/acme/maestro/issues/142`}/>
            <div className="help">Free-text prompt, an issue link, or a mix.</div>
          </div>
          <div className="accord">
            <div className="accord-head" onClick={() => setVarsOpen(!varsOpen)}>
              <span style={{color: 'var(--fg-3)'}} className={varsOpen ? '' : 'collapsed'}><Ic.Chevron/></span>
              <span>Variable overrides</span>
              <span className="acc-count">2 overridden</span>
            </div>
            {varsOpen && (
              <div className="accord-body">
                <div className="var-row" style={{gridTemplateColumns: '110px 1fr', gap: 8}}>
                  <span className="mono" style={{color: 'var(--fg-3)', fontSize: 11.5}}>max_iter</span>
                  <input className="input mono" defaultValue="8" style={{borderColor: 'var(--acc-border)'}}/>
                </div>
                <div className="var-row" style={{gridTemplateColumns: '110px 1fr', gap: 8}}>
                  <span className="mono" style={{color: 'var(--fg-3)', fontSize: 11.5}}>branch_prefix</span>
                  <input className="input mono" defaultValue="exp/" style={{borderColor: 'var(--acc-border)'}}/>
                </div>
                <div className="var-row" style={{gridTemplateColumns: '110px 1fr', gap: 8}}>
                  <span className="mono" style={{color: 'var(--fg-4)', fontSize: 11.5}}>auto_pr</span>
                  <input className="input mono" defaultValue="true"/>
                </div>
                <div className="var-row" style={{gridTemplateColumns: '110px 1fr', gap: 8}}>
                  <span className="mono" style={{color: 'var(--fg-4)', fontSize: 11.5}}>reviewers</span>
                  <input className="input mono" defaultValue="[strict]"/>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary"><Ic.Spark/> Launch</button>
        </div>
      </div>
    </div>
  );
}

function TabBar({ tabs, activeId, onSelect, onClose, dirty = false, savedAt = null, onSave }) {
  const anyDirty = dirty || tabs.some(t => t.dirty);
  return (
    <div className="tabs">
      {tabs.map(t => (
        <div key={t.id} className={"tab" + (t.id === activeId ? " active" : "")} onClick={() => onSelect(t.id)}>
          {t.dirty && <span className="dirty-dot mono">•</span>}
          <span className="mono">{t.id}.yaml</span>
          <span className="x" onClick={(e)=>{e.stopPropagation(); onClose && onClose(t.id);}}><Ic.X/></span>
        </div>
      ))}
      <div className="tabs-save">
        {savedAt && <span className="saved-stamp">Saved {savedAt}</span>}
        <button className={"icon-btn save-icon" + (anyDirty ? ' dirty' : '')}
          disabled={!anyDirty}
          onClick={onSave}
          title={anyDirty ? 'Save' : 'Saved'}>
          <Ic.Floppy/>
        </button>
      </div>
    </div>
  );
}

function EditToolbar({ libraryOpen, onToggleLibrary }) {
  return (
    <div className="edit-toolbar">
      <button title="Add node">
        <Ic.PlusSm/>
        <span className="et-tip">Add node · N</span>
      </button>
      <span className="et-divider"/>
      <button className={libraryOpen ? 'active' : ''} title="Library" onClick={onToggleLibrary}>
        <Ic.Library/>
        <span className="et-tip">Library · L</span>
      </button>
      <button title="Loop">
        <Ic.Loop/>
        <span className="et-tip">Loop node</span>
      </button>
      <button title="Switch">
        <Ic.Switch/>
        <span className="et-tip">Switch node</span>
      </button>
    </div>
  );
}

function EmptyRuns({ onNewRun }) {
  return (
    <div className="empty">
      <div className="emp-art"><Ic.Pulse/></div>
      <div className="emp-title">No runs yet</div>
      <div className="emp-sub">Launch a pipeline to spawn agents in dedicated worktrees and watch them work.</div>
      <button className="btn primary" onClick={onNewRun}><Ic.PlusSm/> New Run</button>
    </div>
  );
}

function EmptyPipelines() {
  return (
    <div className="empty">
      <div className="emp-art"><Ic.Branch/></div>
      <div className="emp-title">No pipelines yet</div>
      <div className="emp-sub">Create a pipeline or import a YAML to get started. Repo pipelines live alongside your code; user pipelines stay on this machine.</div>
      <div style={{display: 'flex', gap: 8}}>
        <button className="btn primary"><Ic.PlusSm/> New Pipeline</button>
        <button className="btn">Import YAML…</button>
      </div>
    </div>
  );
}

window.NewRunModal = NewRunModal;
window.TabBar = TabBar;
window.EditToolbar = EditToolbar;
window.EmptyRuns = EmptyRuns;
window.EmptyPipelines = EmptyPipelines;
