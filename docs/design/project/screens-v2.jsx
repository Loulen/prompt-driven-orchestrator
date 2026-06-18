// screens-v2.jsx — new screens for the unified-mode iteration.
// Composes inline xterm.js terminals, fused Runs+Library left panel,
// pipeline info panel with Manager terminal, ForEach + Merge nodes,
// output-schema editor, frontmatter retry banner, etc.

// ─────────── Helper: inline xterm.js terminal (typeable, scrollback) ───────────

function XTerm({ session = 'pdo/run-a3f/impl · 80×24', height = 220, expanded = false, onExpand, onDetach, lines, awaiting, focused = true, scrolled = false }) {
  const def = lines || [
    { p: '$ ', t: 'tail -F .pdo/runs/a3f/impl/iter-2/log' },
    { d: '[14:32:08] worker started · pid 48211' },
    { p: 'claude › ', t: 'reading plan.md' },
    { d: '  ↳ 247 lines, last edited 4 m ago' },
    { p: 'claude › ', t: 'scanning src/filters/' },
    { d: '  ↳ 12 files matched · 2 modified' },
    { tool: 'edit_file', arg: 'src/filters/archived.ts' },
    { ok: '  ✓ patch applied (+47, -12)' },
    { tool: 'bash', arg: 'pnpm test -- archived.test.ts --watch' },
    { d: '  PASS  src/filters/archived.test.ts' },
    { d: '    ✓ filters by deletedAt (12 ms)' },
    { d: '    ✓ excludes parent of archived (8 ms)' },
    { p: 'claude › ', t: 'writing diff.md', cursor: true },
  ];
  return (
    <div className={"xterm-wrap" + (expanded ? ' expanded' : '')} style={!expanded ? { height } : null}>
      <div className="term-toolbar">
        <span className="tt-dot live"/>
        <span className="mono">{session}</span>
        <span className={"tt-pill " + (focused ? 'live' : '')}>{focused ? 'attached · live' : 'idle'}</span>
        <span className="spacer"/>
        <button className="tt-icon" title={expanded ? 'Collapse terminal' : 'Expand terminal'} onClick={onExpand}>
          {expanded ? <Ic.Minimize/> : <Ic.Maximize/>}
        </button>
        <button className="tt-icon" title="Detach to OS terminal" onClick={onDetach}>
          <Ic.External/>
        </button>
      </div>
      <div className="xterm-body">
        {def.map((l, i) => {
          if (l.tool) return <div key={i} className="term-line"><span className="term-blue">{l.tool}</span> <span className="term-dim">{l.arg}</span></div>;
          if (l.ok) return <div key={i} className="term-line term-ok">{l.ok}</div>;
          if (l.err) return <div key={i} className="term-line term-err">{l.err}</div>;
          if (l.warn) return <div key={i} className="term-line" style={{color:'var(--st-await)'}}>{l.warn}</div>;
          if (l.d) return <div key={i} className="term-line term-dim">{l.d}</div>;
          return <div key={i} className="term-line">{l.p && <span className="term-prompt">{l.p}</span>}{l.t}{l.cursor && <span className="term-cursor"/>}</div>;
        })}
        {awaiting && (
          <div className="term-line" style={{color:'var(--st-await)'}}>
            <span className="term-prompt" style={{color:'var(--st-await)'}}>user › </span>
            <span className="term-cursor" style={{background:'var(--st-await)'}}/>
          </div>
        )}
        {scrolled && (
          <button className="term-pin" title="Scroll to live tail">
            <svg width="12" height="12" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M7 2v9M3 8l4 4 4-4"/></svg>
            jump to live
          </button>
        )}
      </div>
    </div>
  );
}

// ─────────── Helper: fused left panel (Runs + Library tabs) ───────────

const STARRED_TEMPLATES = [
  { id: 'feature-with-review', star: true, nodes: 5, modified: '2 d ago' },
  { id: 'bug-triage', star: true, nodes: 4, modified: '5 d ago' },
  { id: 'doc-refresh', star: false, nodes: 3, modified: '1 wk ago' },
  { id: 'security-audit', star: true, nodes: 6, modified: '2 wk ago' },
  { id: 'release-notes', star: false, nodes: 3, modified: '3 wk ago' },
];

const REUSABLE_NODES = [
  { id: 'impl', name: 'Implementer', kind: 'code', subtitle: 'edit_file · bash · git' },
  { id: 'review', name: 'Reviewer', kind: 'doc', subtitle: 'verdict + feedback' },
  { id: 'plan', name: 'Planner', kind: 'doc', subtitle: 'reads issue · writes plan.md' },
  { id: 'tests', name: 'Tests', kind: 'code', subtitle: 'pnpm/npm/cargo runner' },
  { id: 'merge-resolver', name: 'Merge resolver', kind: 'code', subtitle: 'three-way merge of branches' },
];

function UnifiedLeftPanel({ tab = 'runs', selectedRunId, selectedTemplateId, onSelectRun, onSelectTemplate, onTabChange, dragNodeId, libraryFocus = 'templates' }) {
  return (
    <>
      <PanelHead title={tab === 'runs' ? 'Runs' : 'Library'} count={tab === 'runs' ? RUNS.length : null}
        actions={tab === 'runs'
          ? <button className="btn primary sm"><Ic.PlusSm/> New Run</button>
          : <button className="btn sm"><Ic.PlusSm/> New</button>}/>
      <div className="lp-tabs">
        <button className={"lp-tab" + (tab === 'runs' ? ' on' : '')} onClick={() => onTabChange && onTabChange('runs')}>
          <Ic.Play/> Runs <span className="lp-tab-c">{RUNS.length}</span>
        </button>
        <button className={"lp-tab" + (tab === 'library' ? ' on' : '')} onClick={() => onTabChange && onTabChange('library')}>
          <Ic.Bookmark/> Library
        </button>
      </div>
      {tab === 'runs' && (
        <>
          <div style={{padding: '8px 12px', display: 'flex', gap: 6, borderBottom: '1px solid var(--line-soft)'}}>
            {['All','Active','Done','Failed','Archived'].map((f, i) => (
              <button key={f} className="filter-chip" style={i === 0 ? {color: 'var(--fg)', borderColor: 'var(--bg-5)', background: 'var(--bg-3)'} : {}}>{f}</button>
            ))}
          </div>
          <div className="p-body">
            <div className="runs-list">
              {RUNS.map(r => (
                <div key={r.id} className={"run-row" + (r.id === selectedRunId ? " selected" : "")} onClick={() => onSelectRun && onSelectRun(r.id)}>
                  <span className={"st-dot " + r.status}/>
                  <div className="rr-main">
                    <div className="rr-name">{r.pipeline}</div>
                    <div className="rr-sub">{r.title}</div>
                    <div className="rr-time">{r.when} · {r.elapsed}</div>
                  </div>
                  <button className="icon-btn" style={{width: 22, height: 22}}><Ic.Kebab/></button>
                </div>
              ))}
            </div>
          </div>
        </>
      )}
      {tab === 'library' && (
        <div className="p-body">
          <div className="lib-section">
            <div className="lib-section-head">
              <span className={"chev" + (libraryFocus === 'templates' ? '' : ' collapsed')}><Ic.Chevron/></span>
              Pipeline templates
              <span className="count">· {STARRED_TEMPLATES.length}</span>
              <span className="spacer"/>
              <button className="lib-add" title="New template"><Ic.PlusSm/></button>
            </div>
            <div className="lib-list">
              {STARRED_TEMPLATES.map(t => (
                <div key={t.id} className={"lib-row" + (t.id === selectedTemplateId ? " selected" : "")} onClick={() => onSelectTemplate && onSelectTemplate(t.id)}>
                  <button className={"ih-star sm " + (t.star ? 'synced' : 'outline')} onClick={(e)=>e.stopPropagation()}>
                    {t.star ? <Ic.StarFill/> : <Ic.Star/>}
                  </button>
                  <div className="lib-row-main">
                    <div className="lib-row-name">{t.id}</div>
                    <div className="lib-row-sub">{t.nodes} nodes · {t.modified}</div>
                  </div>
                  <button className="icon-btn" style={{width: 22, height: 22}}><Ic.Kebab/></button>
                </div>
              ))}
            </div>
          </div>
          <div className="lib-section">
            <div className="lib-section-head">
              <span className="chev"><Ic.Chevron/></span>
              Reusable nodes
              <span className="count">· {REUSABLE_NODES.length}</span>
              <span className="spacer"/>
              <button className="lib-add" title="New node"><Ic.PlusSm/></button>
            </div>
            <div className="lib-list">
              {REUSABLE_NODES.map(n => (
                <div key={n.id} className={"lib-row drag" + (n.id === dragNodeId ? ' dragging' : '')}>
                  <span className={"badge " + (n.kind === 'code' ? 'code' : 'doc')} style={{minWidth: 36, justifyContent:'center'}}>{n.kind}</span>
                  <div className="lib-row-main">
                    <div className="lib-row-name">{n.name}</div>
                    <div className="lib-row-sub mono">{n.subtitle}</div>
                  </div>
                  <span className="drag-grip" title="Drag onto canvas">
                    <svg width="10" height="14" viewBox="0 0 10 14" fill="currentColor"><circle cx="3" cy="3" r="1"/><circle cx="3" cy="7" r="1"/><circle cx="3" cy="11" r="1"/><circle cx="7" cy="3" r="1"/><circle cx="7" cy="7" r="1"/><circle cx="7" cy="11" r="1"/></svg>
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

// ─────────── Helper: simplified Run overlay — no Edit-this-run button ───────────

function RunOverlayV2({ run, blocked = false, linkedTemplate = null }) {
  const terminal = run.status === 'done' || run.status === 'failed' || run.status === 'archived';
  return (
    <div className="run-overlay">
      <div className="ro-head">
        <span className={"st-dot " + run.status}/>
        <span className="ro-title">{run.pipeline}</span>
        <span className={"badge " + (run.status === 'running' ? 'running' : run.status === 'blocked' ? 'blocked' : run.status === 'awaiting_user' ? 'awaiting' : run.status === 'failed' ? 'failed' : 'done')}>
          {run.status === 'awaiting_user' ? 'awaiting' : run.status}
        </span>
      </div>
      <div className="ro-row"><span className="ro-label">run-id</span><span className="ro-id">{run.id.slice(-12)} <Ic.Copy className="copy-icon"/></span></div>
      <div className="ro-row"><span className="ro-label">version</span><span className="ro-value mono">v3</span></div>
      <div className="ro-row"><span className="ro-label">started</span><span className="ro-value mono">{run.when}</span></div>
      <div className="ro-row"><span className="ro-label">elapsed</span><span className="ro-value mono" style={run.status === 'running' ? {color: 'var(--st-running)'} : {}}>{run.elapsed}</span></div>
      {run.iter && <div className="ro-row"><span className="ro-label">iter</span><span className="ro-value mono">{run.iter}/5</span></div>}
      <div className="ro-row"><span className="ro-label">vars</span><span className="ro-value mono" style={{color: 'var(--fg-3)'}}>3 set →</span></div>
      {linkedTemplate && (
        <div className="ro-link-tpl">
          <Ic.Bookmark/>
          <span>linked to template <span className="mono" style={{color:'var(--fg-2)'}}>{linkedTemplate}</span></span>
          <Ic.ArrowRight/>
        </div>
      )}
      {blocked && (
        <div className="halt-callout">
          <div className="hc-title"><Ic.Halt/> halted</div>
          Max iterations reached without PASS verdict. Open the manager to extend the cycle or mark the run done.
        </div>
      )}
      <div className="ro-actions-col" style={{marginTop: 12}}>
        <button className="btn"><Ic.Manager/> Open Manager</button>
        {run.status === 'running' && <button className="btn warn"><Ic.X/> Cancel</button>}
        {terminal && <button className="btn">Cleanup</button>}
      </div>
    </div>
  );
}

// ─────────── Helper: canvas toolbar with `i` button ───────────

function CanvasToolbar({ activeTool = null, onTool, infoOpen = false }) {
  const tools = [
    { id: 'select', icon: <Ic.Cursor/>, label: 'Select' },
    { id: 'code', icon: <Ic.Code/>, label: 'Code node' },
    { id: 'doc', icon: <Ic.Doc/>, label: 'Doc node' },
    { id: 'foreach', icon: <IcForEach/>, label: 'ForEach' },
    { id: 'merge', icon: <IcMerge/>, label: 'Merge' },
    { id: 'loop', icon: <Ic.Loop/>, label: 'Loop' },
    { id: 'switch', icon: <Ic.Switch/>, label: 'Switch' },
  ];
  return (
    <div className="canvas-toolbar">
      {tools.map(t => (
        <button key={t.id} className={"ct-btn" + (activeTool === t.id ? ' on' : '')} onClick={() => onTool && onTool(t.id)} title={t.label}>
          {t.icon}
        </button>
      ))}
      <span className="ct-sep"/>
      <button className={"ct-btn" + (infoOpen ? ' on' : '')} title="Pipeline info" onClick={() => onTool && onTool('info')}>
        <Ic.Info/>
      </button>
    </div>
  );
}

// ─────────── Helper: ForEach + Merge node icons (toolbar) ───────────

function IcForEach() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
      <circle cx="3" cy="7" r="1.6"/>
      <path d="M4.6 7L7 4M4.6 7L7 7M4.6 7L7 10"/>
      <circle cx="9" cy="4" r="1.4"/>
      <circle cx="9" cy="7" r="1.4"/>
      <circle cx="9" cy="10" r="1.4"/>
    </svg>
  );
}
function IcMerge() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
      <circle cx="3" cy="3" r="1.4"/>
      <circle cx="3" cy="7" r="1.4"/>
      <circle cx="3" cy="11" r="1.4"/>
      <path d="M4.6 3L7 7M4.6 7L7 7M4.6 11L7 7"/>
      <circle cx="11" cy="7" r="1.6"/>
      <path d="M8.6 7H9.4"/>
    </svg>
  );
}

// ─────────── Helper: ForEach node ───────────

function ForEachNode({ x, y, status = 'pending', selected, items = 4, currentIter = null }) {
  return (
    <div className={"node node-foreach " + status + (selected ? ' selected' : '')} style={{ left: x, top: y, width: 200 }}>
      <div className="node-head">
        <span className={"st-dot " + status}/>
        <span className="node-name">ForEach</span>
        {currentIter != null && <span className="node-iter mono">{currentIter}/{items}</span>}
      </div>
      <div className="mono" style={{fontSize: 9, color: 'var(--fg-4)', marginTop: -2}}>fe9k2x · items: $plan.subtasks</div>
      <div className="fe-fanout">
        {Array.from({length: Math.min(items, 4)}).map((_, i) => (
          <div key={i} className="fe-pip" style={{opacity: status === 'pending' ? 0.5 : 1}}/>
        ))}
        <span className="fe-fanout-label mono">×{items}</span>
      </div>
      <div className="node-meta">
        <span className="badge">control</span>
        <span className="node-status mono">{status === 'running' ? '· fanning out' : status === 'done' ? '· joined' : '· pending'}</span>
      </div>
      {/* ports: in (left top), break (left bottom), body (right top), done (right bottom) */}
      <span className="tri-handle side-left" style={{ left: -7, top: '28%' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
      <span className="tri-handle side-left" style={{ left: -7, top: '70%' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
      <span className="tri-handle side-right" style={{ right: -7, top: '28%' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
      <span className="tri-handle side-right" style={{ right: -7, top: '70%' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
      <span className="port-tag tag-l" style={{ top: 'calc(28% - 6px)' }}>in</span>
      <span className="port-tag tag-l" style={{ top: 'calc(70% - 6px)' }}>break</span>
      <span className="port-tag tag-r" style={{ top: 'calc(28% - 6px)' }}>body</span>
      <span className="port-tag tag-r" style={{ top: 'calc(70% - 6px)' }}>done</span>
    </div>
  );
}

// ─────────── Helper: Merge node ───────────

function MergeNode({ x, y, status = 'pending', selected, branches = 3 }) {
  return (
    <div className={"node node-merge " + status + (selected ? ' selected' : '')} style={{ left: x, top: y, width: 200 }}>
      <div className="node-head">
        <span className={"st-dot " + status}/>
        <span className="node-name">Merge</span>
      </div>
      <div className="mono" style={{fontSize: 9, color: 'var(--fg-4)', marginTop: -2}}>mg7h1v · 3-way · code-mutating</div>
      <div className="merge-funnel">
        <svg width="160" height="22" viewBox="0 0 160 22" fill="none">
          {[3, 11, 19].map((y, i) => (
            <path key={i} d={`M0 ${y} C 60 ${y}, 80 11, 130 11`} stroke="rgba(245,158,11,0.55)" strokeWidth="1.2" strokeDasharray="3 2"/>
          ))}
          <circle cx="132" cy="11" r="3" fill="var(--st-await)"/>
        </svg>
        <span className="fe-fanout-label mono">{branches}→1</span>
      </div>
      <div className="node-meta">
        <span className="badge code"><Ic.Code/>code</span>
        <span className="node-status mono">{status === 'running' ? '· merging' : status === 'done' ? '· merged' : '· pending'}</span>
      </div>
      <span className="tri-handle side-left" style={{ left: -7, top: '50%', transform: 'translateY(-50%)' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
      <span className="tri-handle side-right" style={{ right: -7, top: '50%', transform: 'translateY(-50%)' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
      <span className="port-tag tag-l" style={{ top: 'calc(50% - 6px)' }}>branches <span className="prep">⤺</span></span>
      <span className="port-tag tag-r" style={{ top: 'calc(50% - 6px)' }}>merged</span>
    </div>
  );
}

// ─────────── Helper: NodeDetail v2 — inline xterm + retry banner ───────────

function NodeDetailV2({ node, retry = false, failedValidation = false, expanded = false, onExpand, hideStar = false, libState = 'outline' }) {
  return (
    <div className="p-body">
      {!expanded && (
        <div className="p-sect" style={{paddingBottom: 10}}>
          <div style={{display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8}}>
            <span className={"st-dot " + node.status}/>
            <div style={{flex: 1, minWidth: 0}}>
              <div style={{fontSize: 13, fontWeight: 600}}>{node.name}</div>
              <div style={{fontSize: 11, color: 'var(--fg-3)', marginTop: 2}} className="mono">{node.id}</div>
            </div>
            <span className={"badge " + (node.type === 'code' ? 'code' : 'doc')}>{node.type === 'code' ? 'code' : 'doc'}</span>
            {node.iter && <span className="badge">iter {node.iter}</span>}
            {!hideStar && (
              <button className={"ih-star " + libState}>
                {libState === 'outline' ? <Ic.Star/> : <Ic.StarFill/>}
                {libState === 'diverged' && <span className="ih-star-notch"/>}
              </button>
            )}
          </div>
        </div>
      )}

      {retry && (
        <div className="p-sect" style={{paddingTop: 8, paddingBottom: 8}}>
          <div className="retry-banner">
            <div className="rb-title"><Ic.Refresh/> awaiting frontmatter retry</div>
            <div className="rb-body">
              Agent prompted to fix <span className="mono">files_changed: expected int, got "seven"</span>.
              <div className="rb-meta mono">retry 1 of 1 · prompted via tmux at 14:32:08</div>
            </div>
          </div>
        </div>
      )}

      {failedValidation && (
        <div className="p-sect" style={{paddingTop: 8, paddingBottom: 8}}>
          <div className="fail-banner">
            <div className="fb-title"><Ic.Warn/> failed</div>
            output validation failed — agent could not produce a frontmatter that matches the schema after one retry.
            <div className="fb-meta">iter 3/5 · failed at 14:33:42</div>
          </div>
          <div style={{display: 'flex', gap: 6, marginTop: 8}}>
            <button className="btn primary sm" style={{flex: 1, justifyContent: 'center'}}><Ic.Check/> Mark complete</button>
            <button className="btn sm"><Ic.Terminal/> Detach to OS</button>
          </div>
          <div className="fail-subbanner">
            <span className="fsb-label">409</span>
            <div>
              <div style={{marginBottom: 3}}>output validation — 2 fields invalid:</div>
              <div className="mono fsb-row"><span className="fsb-k">files_changed</span> expected <span className="fsb-t">int</span>, got <span className="fsb-bad">"seven"</span></div>
              <div className="mono fsb-row"><span className="fsb-k">verdict</span> expected <span className="fsb-t">enum[PASS,FAIL,NEEDS_WORK]</span>, got <span className="fsb-bad">"ok"</span></div>
            </div>
          </div>
        </div>
      )}

      <div className="p-sect" style={expanded ? {padding: '8px 10px', borderBottom: 'none'} : null}>
        {!expanded && <SectionHead title="Terminal"/>}
        <XTerm height={expanded ? null : 220} expanded={expanded} onExpand={onExpand}
          session={`tmux: pdo/${node.runSlug || 'run-a3f'}/${node.id} · 80×24`}
          awaiting={node.status === 'awaiting_user'}/>
      </div>

      {expanded && (
        <div className="exp-strip">
          <SectionHead title="Inputs" count={2} collapsed/>
          <SectionHead title="Outputs" count={1} collapsed/>
          <SectionHead title="Initial prompt" collapsed/>
        </div>
      )}

      {!expanded && (
        <>
          <div className="p-sect">
            <SectionHead title="Inputs" count={2}/>
            <div className="port-row">
              <span className="pdot ok"/>
              <div style={{minWidth: 0}}>
                <div className="pname">plan</div>
                <div className="ppath">artifacts/plan/plan.md</div>
              </div>
              <span className="open-link">open ↗</span>
            </div>
            <div className="port-row">
              <span className="pdot accumulating"/>
              <div style={{minWidth: 0}}>
                <div className="pname">review_feedback <span className="badge" style={{marginLeft: 4, height: 14, padding: '0 4px', fontSize: 9}}>repeated</span></div>
                <div className="ppath">artifacts/review/iter-*/feedback.md</div>
              </div>
              <span className="pmeta">2 files</span>
            </div>
          </div>
          <div className="p-sect">
            <SectionHead title="Outputs" count={1}/>
            <div className="port-row">
              <span className="pdot ok"/>
              <div style={{minWidth: 0}}>
                <div className="pname">diff</div>
                <div className="ppath">artifacts/impl/iter-2/diff.md</div>
              </div>
              <span className="pmeta">3.2 KB</span>
            </div>
            <div className="frontmatter">
              <span className="k">summary</span><span className="v">"add archived filter"</span>
              <span className="k">files_changed</span><span className="v">7</span>
              <span className="k">tests_added</span><span className="v">3</span>
            </div>
          </div>
          <div className="p-sect">
            <SectionHead title="Initial prompt" collapsed/>
          </div>
        </>
      )}
    </div>
  );
}

// ─────────── Helper: Pipeline-info panel (toolbar `i` button) ───────────

function PipelineInfoPanel({ run = null, starState = 'synced', popoverOpen = false, idle = false }) {
  return (
    <div className="p-body">
      <div className="p-sect pip-meta">
        <div className="pip-head">
          <span className={"st-dot " + (run ? run.status : 'pending')}/>
          <div style={{flex: 1, minWidth: 0}}>
            <div className="pip-name">feature-with-review</div>
            <div className="pip-sub mono">{run ? `run ${run.id.slice(-8)} · v3` : 'template · v3'}</div>
          </div>
          <button className={"ih-star " + starState} style={{position:'relative'}}>
            {starState === 'outline' ? <Ic.Star/> : <Ic.StarFill/>}
            {starState === 'diverged' && <span className="ih-star-notch"/>}
            {popoverOpen && (
              <div className="ih-star-pop" onClick={(e)=>e.stopPropagation()}>
                <div className="ihp-title">{starState === 'diverged' ? 'Out of sync with library' : 'In your library'}</div>
                {starState === 'diverged' && (
                  <>
                    <div className="ihp-action">
                      <Ic.Spark/> <span><b>Update library entry</b><span className="ihp-sub">Push this run's pipeline back to the template.</span></span>
                    </div>
                    <div className="ihp-action">
                      <Ic.Branch/> <span><b>Reset from library</b><span className="ihp-sub">Discard local pipeline edits; sync from template.</span></span>
                    </div>
                  </>
                )}
                <div className="ihp-action danger">
                  <Ic.Trash/> <span><b>Remove from library</b><span className="ihp-sub">This pipeline will no longer appear in the +&nbsp;New&nbsp;Run picker.</span></span>
                </div>
              </div>
            )}
          </button>
        </div>
        <div className="pip-vars">
          <div className="pip-var"><span className="k mono">max_iter</span><span className="v mono">5</span></div>
          <div className="pip-var"><span className="k mono">branch_prefix</span><span className="v mono">"feat/"</span></div>
          <div className="pip-var"><span className="k mono">auto_pr</span><span className="v mono">true</span></div>
          <div className="pip-var"><span className="k mono">reviewers</span><span className="v mono">[strict]</span></div>
        </div>
      </div>

      {!idle && run && (
        <div className="p-sect" style={{padding: '12px 12px 16px', borderBottom: 'none', flex: 1, display: 'flex', flexDirection: 'column'}}>
          <div className="pip-mgr-head">
            <Ic.Manager/>
            <span>Pipeline Manager</span>
            <span className="mono" style={{color:'var(--fg-4)', fontSize: 10.5}}>pdo-mgr-{run.id.slice(-8)}</span>
          </div>
          <XTerm session={`tmux: pdo-mgr-${run.id.slice(-8)} · 100×30`}
            height={420}
            lines={[
              { d: '[14:28:01] manager session attached · 4 worker(s)' },
              { p: 'mgr › ', t: 'scheduler tick' },
              { d: '  ↳ plan: done' },
              { d: '  ↳ impl: running · iter 2/5' },
              { d: '  ↳ review: running · iter 2/5' },
              { d: '  ↳ tests: pending (waiting on impl.diff)' },
              { d: '  ↳ merge: pending (waiting on review.verdict, tests.result)' },
              { p: 'mgr › ', t: 'next tick in 6 s · idle' },
              { tool: 'cmd', arg: 'extend_cycle review --by 2' },
              { ok: '  → max_iter: 5 → 7 · applied to live run' },
              { p: 'mgr › ', t: '', cursor: true },
            ]}/>
        </div>
      )}

      {idle && (
        <div className="p-sect" style={{padding: '14px 16px'}}>
          <SectionHead title="Description"/>
          <div style={{fontSize: 12, color: 'var(--fg-3)', lineHeight: 1.55, marginTop: 6}}>
            Plan → implement → review loop, tests, then merge. Halts on max iterations.
          </div>
          <div style={{marginTop: 14, padding: '10px 12px', background: 'var(--bg-3)', borderRadius: 6, border: '1px dashed var(--line-soft)', display: 'flex', alignItems: 'center', gap: 8, color: 'var(--fg-4)', fontSize: 11.5}}>
            <Ic.Info/> No active run. Manager terminal appears here while a Run is in progress.
          </div>
        </div>
      )}
    </div>
  );
}

// ─────────── Helper: Output schema editor (typed output rows) ───────────

function OutputSchemaEditor({ enumOpen = false, focusEnum = false }) {
  return (
    <div className="schema-port">
      <div className="schema-port-head">
        <span className="pdot"/>
        <span className="pname">verdict</span>
        <SidePicker value="right"/>
        <button className="icon-btn" style={{width: 22, height: 22}}><Ic.Kebab/></button>
      </div>
      <div className="schema-rows">
        <div className="schema-row">
          <input className="input mono sm" defaultValue="summary"/>
          <select className="select mono sm" defaultValue="string"><option>string</option><option>int</option><option>bool</option><option>list</option><option>enum</option></select>
          <button className="icon-btn" style={{width: 22, height: 22}}><Ic.X/></button>
        </div>
        <div className="schema-row">
          <input className="input mono sm" defaultValue="files_changed"/>
          <select className="select mono sm" defaultValue="int"><option>int</option><option>string</option><option>bool</option><option>list</option><option>enum</option></select>
          <button className="icon-btn" style={{width: 22, height: 22}}><Ic.X/></button>
        </div>
        <div className={"schema-row enum-row" + (focusEnum ? ' focused' : '')}>
          <input className="input mono sm" defaultValue="verdict"/>
          <select className={"select mono sm" + (enumOpen ? ' open' : '')} defaultValue="enum">
            <option>enum</option><option>string</option><option>int</option><option>bool</option><option>list</option>
          </select>
          <button className="icon-btn" style={{width: 22, height: 22}}><Ic.X/></button>
          <div className="enum-allowed">
            <span className="ea-label mono">allowed:</span>
            <span className="ea-chip">PASS <button>×</button></span>
            <span className="ea-chip">FAIL <button>×</button></span>
            <span className="ea-chip">NEEDS_WORK <button>×</button></span>
            <button className="ea-add">+ value</button>
          </div>
          <div className="enum-help">runtime validates the agent's frontmatter against this set; wrong values trigger one in-tmux retry.</div>
          {enumOpen && (
            <div className="type-menu">
              {['string', 'int', 'bool', 'list', 'enum'].map(t => (
                <button key={t} className={t === 'enum' ? 'on' : ''}>
                  <span className="tm-name mono">{t}</span>
                  <span className="tm-hint">
                    {t === 'string' && 'free text'}
                    {t === 'int' && 'integer'}
                    {t === 'bool' && 'true / false'}
                    {t === 'list' && 'YAML array'}
                    {t === 'enum' && 'closed set + allowed:[…]'}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
      <button className="btn ghost sm" style={{marginTop: 6}}><Ic.PlusSm/> Add field</button>
    </div>
  );
}

// ─────────── Helper: NodeInspectorEdit v2 — typed Outputs ───────────

function NodeInspectorEditV2({ enumOpen = false, focusEnum = false, schemaOnly = false }) {
  return (
    <div className="p-body">
      {!schemaOnly && (
        <>
          <div className="p-sect">
            <SectionHead title="Identity"/>
            <div className="field">
              <label>id <span style={{color:'var(--fg-4)', fontWeight:400}}>· immutable</span></label>
              <div className="input mono" style={{background:'var(--bg-0)', color:'var(--fg-3)'}}>q4n8jp</div>
            </div>
            <div className="field">
              <label>Display name</label>
              <input className="input" defaultValue="Reviewer"/>
            </div>
          </div>
          <div className="p-sect">
            <SectionHead title="Type" collapsed/>
          </div>
          <div className="p-sect">
            <SectionHead title="Behavior" collapsed/>
          </div>
          <div className="p-sect">
            <SectionHead title="Prompt" collapsed/>
          </div>
          <div className="p-sect">
            <SectionHead title="Inputs" count={1}/>
            <div className="port-row" style={{gridTemplateColumns: '12px 1fr 80px auto'}}>
              <span className="pdot"/>
              <div>
                <div className="pname">diff</div>
                <div className="help" style={{marginTop: 2}}>repeated: off · <span style={{color:'var(--fg-4)'}}>untyped — shape inferred from upstream</span></div>
              </div>
              <SidePicker value="left"/>
              <button className="icon-btn" style={{width: 22, height: 22}}><Ic.Kebab/></button>
            </div>
            <button className="btn ghost sm" style={{marginTop: 6}}><Ic.PlusSm/> Add input port</button>
          </div>
        </>
      )}
      <div className="p-sect">
        <SectionHead title="Outputs" count={2}/>
        <OutputSchemaEditor enumOpen={enumOpen} focusEnum={focusEnum}/>
        {!schemaOnly && (
          <div className="schema-port" style={{marginTop: 10}}>
            <div className="schema-port-head">
              <span className="pdot"/>
              <span className="pname">review_feedback</span>
              <SidePicker value="top"/>
              <button className="icon-btn" style={{width: 22, height: 22}}><Ic.Kebab/></button>
            </div>
            <div className="schema-rows">
              <div className="schema-row">
                <input className="input mono sm" defaultValue="comments"/>
                <select className="select mono sm" defaultValue="string"><option>string</option></select>
                <button className="icon-btn" style={{width: 22, height: 22}}><Ic.X/></button>
              </div>
            </div>
            <button className="btn ghost sm" style={{marginTop: 6}}><Ic.PlusSm/> Add field</button>
          </div>
        )}
      </div>
    </div>
  );
}

// ─────────── Lint banner (info-only diagnostic on canvas) ───────────

function LintBanner({ x = 540, y = 60 }) {
  return (
    <div className="lint-banner" style={{ left: x, top: y }}>
      <div className="lb-mark"><Ic.Info/></div>
      <div className="lb-body">
        <div className="lb-title">Fan-out without a Merge</div>
        <div className="lb-text">
          Two <span className="mono">code-mutating</span> nodes branch from <span className="mono">plan</span> but never converge.
          Add a <span className="mono">Merge</span> node downstream to combine their branches; otherwise their commits stay on separate worktrees.
        </div>
        <div className="lb-actions">
          <button className="btn sm primary"><Ic.PlusSm/> Insert Merge</button>
          <button className="btn ghost sm">Dismiss</button>
        </div>
      </div>
      <button className="lb-x" title="Dismiss">×</button>
    </div>
  );
}

// ─────────── Helper: New Run modal v2 (starred templates picker) ───────────

function NewRunModalV2({ open = true, onClose, pickerOpen = false }) {
  if (!open) return null;
  return (
    <div className="modal-bg">
      <div className="modal" style={{width: 520}}>
        <div className="modal-head">
          <h2>New Run</h2>
          <button className="icon-btn" onClick={onClose}><Ic.X/></button>
        </div>
        <div className="modal-body">
          <div className="field">
            <label>Pipeline template</label>
            <div className={"picker" + (pickerOpen ? ' open' : '')}>
              <button className="picker-btn">
                <Ic.StarFill style={{color:'var(--st-await)'}}/>
                <span className="mono" style={{color:'var(--fg)'}}>feature-with-review</span>
                <span className="mono" style={{color:'var(--fg-4)', fontSize: 10.5}}>· 5 nodes · v3</span>
                <span className="spacer"/>
                <Ic.Chevron style={{transform: 'rotate(-90deg)'}}/>
              </button>
              {pickerOpen && (
                <div className="picker-list">
                  <div className="picker-list-head">Starred templates · 3</div>
                  {STARRED_TEMPLATES.filter(t => t.star).map(t => (
                    <div key={t.id} className={"picker-row" + (t.id === 'feature-with-review' ? ' on' : '')}>
                      <Ic.StarFill style={{color:'var(--st-await)'}}/>
                      <span className="mono" style={{color:'var(--fg)'}}>{t.id}</span>
                      <span className="mono" style={{color:'var(--fg-4)', fontSize: 10.5, marginLeft: 'auto'}}>{t.nodes} nodes · {t.modified}</span>
                    </div>
                  ))}
                  <div className="picker-foot">
                    <Ic.Bookmark/>
                    <span>Only starred templates show here.</span>
                    <a className="picker-link">Manage library →</a>
                  </div>
                </div>
              )}
            </div>
            <div className="help">Star a template in the Library to make it launchable here.</div>
          </div>
          <div className="field">
            <label>Run title</label>
            <input className="input" defaultValue="Implement search filter for archived projects"/>
          </div>
          <div className="vars-group">
            <div className="vars-label">Variables</div>
            <div className="var-row" style={{gridTemplateColumns:'140px 60px 1fr', gap: 8}}>
              <input className="input mono sm" defaultValue="max_iter" readOnly/>
              <input className="input mono sm" defaultValue="int" readOnly/>
              <input className="input mono sm" defaultValue="5"/>
            </div>
            <div className="var-row" style={{gridTemplateColumns:'140px 60px 1fr', gap: 8}}>
              <input className="input mono sm" defaultValue="branch_prefix" readOnly/>
              <input className="input mono sm" defaultValue="str" readOnly/>
              <input className="input mono sm" defaultValue="feat/"/>
            </div>
            <div className="var-row" style={{gridTemplateColumns:'140px 60px 1fr', gap: 8}}>
              <input className="input mono sm" defaultValue="reviewers" readOnly/>
              <input className="input mono sm" defaultValue="list" readOnly/>
              <input className="input mono sm" defaultValue="[strict]"/>
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn">Cancel</button>
          <button className="btn primary"><Ic.Play/> Launch</button>
        </div>
      </div>
    </div>
  );
}

// ────────────────────────────────────────────────────────────────────
// SCREENS
// ────────────────────────────────────────────────────────────────────

const NODE_OFFSET = 200;

function buildRunNodes(opts = {}) {
  const nodes = FWR_NODES.map(n => ({ ...n, x: n.x + NODE_OFFSET }));
  if (opts.failed) {
    return nodes.map(n => {
      if (n.id === 'plan') return { ...n, status: 'done' };
      if (n.id === 'impl') return { ...n, status: 'failed', iter: '3/5' };
      if (n.id === 'review') return { ...n, status: 'pending' };
      return n;
    });
  }
  return nodes;
}

function defaultRunCanvas({ run, sel, setSel, expandedIfSel = null, lint = false, addedNode = null, infoOpen = false, blockedNodeId = null, failedView = false, mergeShown = false }) {
  const nodes = buildRunNodes({ failed: failedView });
  const runningSet = new Set(nodes.filter(n => n.status === 'running' || n.status === 'done').map(n => n.id));
  const startX = 30, startY = 215;
  const fakeStart = { id: '__start', x: startX - 56, y: startY - 35, status: 'running' };
  const fakeEdges = [{ id: 'se-plan', from: '__start', to: 'plan' }];
  return (
    <div className="dag-canvas">
      <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
        <Edges nodes={[...nodes, fakeStart]} edges={[...FWR_EDGES, ...fakeEdges]} runningSet={new Set([...runningSet, '__start'])}/>
        <StartNode x={startX} y={startY} when={run.when} runIdSlug={run.id.slice(-8)}
          selected={sel === '__start'} onSelect={setSel} downstreamRunning/>
        {nodes.map(n => (
          <Node key={n.id} node={n} selected={n.id === sel} onSelect={setSel}/>
        ))}
        {addedNode && (
          <>
            <div className="node pending added-pulse" style={{ left: addedNode.x, top: addedNode.y, width: 200 }} onClick={() => setSel && setSel(addedNode.id)}>
              <div className="node-head">
                <span className="st-dot pending"/>
                <span className="node-name">{addedNode.name}</span>
              </div>
              <div className="mono" style={{fontSize: 9, color: 'var(--fg-4)', marginTop: -2}}>{addedNode.nid} · just added</div>
              <div className="node-meta">
                <span className={"badge " + (addedNode.kind === 'code' ? 'code' : 'doc')}>{addedNode.kind}</span>
                <span className="node-status mono">· pending</span>
              </div>
              <span className="tri-handle side-left" style={{ left: -7, top: '50%' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
              <span className="tri-handle side-right" style={{ right: -7, top: '50%' }}><svg width="13" height="13" viewBox="0 0 13 13"><polygon className="tri" points="2,5 2,11 10,8"/></svg></span>
            </div>
            <svg style={{position:'absolute', inset:0, width:'100%', height:'100%', pointerEvents:'none'}}>
              <path d={`M ${addedNode.from.x} ${addedNode.from.y} C ${addedNode.from.x + 60} ${addedNode.from.y}, ${addedNode.x - 60} ${addedNode.y + 35}, ${addedNode.x} ${addedNode.y + 35}`}
                stroke="var(--acc)" strokeWidth="1.5" strokeDasharray="4 4" fill="none"/>
            </svg>
          </>
        )}
        {blockedNodeId && (() => {
          const n = nodes.find(x => x.id === blockedNodeId);
          if (!n) return null;
          return (
            <div className="lock-bubble" style={{ left: n.x + 100, top: n.y - 18 }}>
              <Ic.Lock/> running · cannot delete
            </div>
          );
        })()}
        <EdgeLabels nodes={nodes} edges={FWR_EDGES}/>
      </div>
      {!infoOpen && <RunOverlayV2 run={run} linkedTemplate="feature-with-review"/>}
      <CanvasToolbar activeTool="select" infoOpen={infoOpen}/>
      <MiniMap nodes={nodes}/>
      <CanvasControls/>
      {lint && <LintBanner/>}
    </div>
  );
}

// 1 ── Run · pipeline running, node terminal at default height
function ScreenA1() {
  const run = RUNS[0];
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={run.id}/></div>
        <div className="panel panel-c">{defaultRunCanvas({ run, sel: 'impl' })}</div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <NodeDetailV2 node={{...FWR_NODES[1], runSlug: 'run-a3f'}} libState="diverged"/>
        </div>
      </Frame>
    </div>
  );
}

// 2 ── Run · pipeline-info panel with Manager terminal dominant
function ScreenA2() {
  const run = RUNS[0];
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={run.id}/></div>
        <div className="panel panel-c">{defaultRunCanvas({ run, sel: null, infoOpen: true })}</div>
        <div className="panel panel-r">
          <PanelHead title="Pipeline info" actions={<button className="icon-btn" title="Close info"><Ic.X/></button>}/>
          <PipelineInfoPanel run={run} starState="synced"/>
        </div>
      </Frame>
    </div>
  );
}

// 3 ── Run · edit during run · new pending node added, running node locked
function ScreenA3() {
  const run = RUNS[0];
  const added = { id: 'doc-update', nid: 'd9k4xn', name: 'Update docs', kind: 'doc', x: 800, y: 470,
    from: { x: 460 + NODE_OFFSET, y: 355 } };
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" libraryFocus="nodes" dragNodeId="plan"/></div>
        <div className="panel panel-c">{defaultRunCanvas({ run, sel: 'impl', addedNode: added, blockedNodeId: 'impl' })}</div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <NodeDetailV2 node={{...FWR_NODES[1], runSlug: 'run-a3f'}} libState="synced"/>
        </div>
      </Frame>
    </div>
  );
}

// 4 ── Run · failed node with frontmatter retry exhausted
function ScreenA4() {
  const run = { ...RUNS[5], status: 'failed' };
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="security-audit" runId={run.id.slice(-8)} awaiting={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={run.id}/></div>
        <div className="panel panel-c">{defaultRunCanvas({ run, sel: 'impl', failedView: true })}</div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <NodeDetailV2 node={{...FWR_NODES[1], runSlug: 'run-2af', status: 'failed', iter: '3/5'}} failedValidation/>
        </div>
      </Frame>
    </div>
  );
}

// 5 ── Library · template selected, canvas in design context
function ScreenB5() {
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" awaiting={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" selectedTemplateId="feature-with-review" libraryFocus="templates"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
              <Edges nodes={[...buildRunNodes(), { id:'__start', x: -26, y: 180, status: 'pending' }]}
                edges={[...FWR_EDGES, { id:'se-plan', from:'__start', to:'plan' }]} runningSet={new Set()}/>
              <StartNode x={30} y={215} when="—" runIdSlug="(no run)"/>
              {buildRunNodes().map(n => (
                <Node key={n.id} node={{...n, status: 'pending', iter: undefined}} selected={n.id === 'review'}/>
              ))}
            </div>
            <CanvasToolbar activeTool="select"/>
            <MiniMap nodes={buildRunNodes().map(n => ({...n, status: 'pending'}))}/>
            <CanvasControls/>
            <div className="design-hint">
              <Ic.Bookmark/>
              <div>
                <div style={{fontWeight: 600, color: 'var(--fg-2)'}}>Template · feature-with-review</div>
                <div style={{color:'var(--fg-4)', marginTop: 2}}>No run attached. Drag from Reusable nodes to extend, or click <span style={{color:'var(--fg-2)'}}>+ New Run</span> to launch.</div>
              </div>
            </div>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Reviewer"/>
          <NodeInspectorEditV2/>
        </div>
      </Frame>
    </div>
  );
}

// 6 ── Node inspector · output schema editor with enum
function ScreenB6() {
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" awaiting={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" selectedTemplateId="feature-with-review" libraryFocus="templates"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
              <StartNode x={30} y={215} when="—" runIdSlug="(no run)"/>
              {buildRunNodes().map(n => (
                <Node key={n.id} node={{...n, status: 'pending', iter: undefined}} selected={n.id === 'review'}/>
              ))}
            </div>
            <CanvasToolbar activeTool="select"/>
            <MiniMap nodes={buildRunNodes().map(n => ({...n, status: 'pending'}))}/>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Reviewer"/>
          <NodeInspectorEditV2 focusEnum/>
        </div>
      </Frame>
    </div>
  );
}

// 7 ── Canvas · ForEach + Merge convergence
function ScreenB7() {
  // graph: plan → ForEach (body → 3 impl nodes → Merge) ; ForEach (done → tests → after-merge)
  const planN = { id: 'plan', nid: 'k7m2x9', name: 'Planner', type: 'doc', status: 'pending', x: 60, y: 220, ports: { in:['issue'], out:['plan'] } };
  const fe = { x: 320, y: 220 };
  const impls = [
    { id: 'i1', nid: '9k2x71', name: 'Implementer', type: 'code', status: 'pending', x: 600, y: 60, ports: { in:['plan'], out:['diff'] } },
    { id: 'i2', nid: '9k2x72', name: 'Implementer', type: 'code', status: 'pending', x: 600, y: 220, ports: { in:['plan'], out:['diff'] } },
    { id: 'i3', nid: '9k2x73', name: 'Implementer', type: 'code', status: 'pending', x: 600, y: 380, ports: { in:['plan'], out:['diff'] } },
  ];
  const merge = { x: 880, y: 220 };
  const tail = { id: 'tail', nid: 'r3w6tz', name: 'Tests', type: 'code', status: 'pending', x: 1140, y: 220, ports: { in:['result'], out:['summary'] } };
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="parallel-pattern" awaiting={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" selectedTemplateId="parallel-pattern" libraryFocus="templates"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
              <Node node={planN}/>
              <ForEachNode x={fe.x} y={fe.y} items={3} status="pending"/>
              {impls.map(i => <Node key={i.id} node={i}/>)}
              <MergeNode x={merge.x} y={merge.y} branches={3} status="pending" selected/>
              <Node node={tail}/>
              {/* edges */}
              <svg style={{position:'absolute', inset:0, width:'100%', height:'100%', pointerEvents:'none'}}>
                <defs>
                  <marker id="arr2" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L8 4 L0 8 z" fill="#525a68"/></marker>
                </defs>
                {/* plan -> ForEach.in */}
                <path d="M 260 255 C 290 255, 290 248, 320 248" stroke="#525a68" strokeWidth="1.5" fill="none" markerEnd="url(#arr2)"/>
                {/* ForEach.body -> each impl */}
                {impls.map((i, idx) => (
                  <path key={idx} d={`M 520 248 C 560 248, 560 ${i.y + 35}, 600 ${i.y + 35}`} stroke="#525a68" strokeWidth="1.5" fill="none" markerEnd="url(#arr2)"/>
                ))}
                {/* each impl -> Merge */}
                {impls.map((i, idx) => (
                  <path key={'m'+idx} d={`M 800 ${i.y + 35} C 840 ${i.y + 35}, 840 255, 880 255`} stroke="#525a68" strokeWidth="1.5" strokeDasharray="3 3" fill="none" markerEnd="url(#arr2)"/>
                ))}
                {/* Merge -> Tests */}
                <path d="M 1080 255 C 1110 255, 1110 255, 1140 255" stroke="#525a68" strokeWidth="1.5" fill="none" markerEnd="url(#arr2)"/>
                {/* ForEach.done loops down past merge to Tests (intrinsic barrier) */}
                <path d="M 520 290 C 600 320, 1000 380, 1080 320 C 1130 290, 1140 270, 1140 260"
                  stroke="#3b82f6aa" strokeWidth="1.4" strokeDasharray="4 3" fill="none" markerEnd="url(#arr2)"/>
              </svg>
              <div className="edge-label cond" style={{ left: 540, top: 360, color:'#93c5fd', borderColor:'rgba(59,130,246,0.32)', background:'rgba(59,130,246,0.10)' }}>done · barrier</div>
              <div className="edge-label" style={{ left: 700, top: 240 }}>×3 parallel · body</div>
            </div>
            <CanvasToolbar activeTool="merge"/>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Merge"/>
          <div className="p-body">
            <div className="p-sect">
              <div style={{display:'flex', alignItems:'center', gap:8, marginBottom:10}}>
                <span className="st-dot pending"/>
                <div style={{flex:1}}>
                  <div style={{fontSize:13, fontWeight:600}}>Merge</div>
                  <div className="mono" style={{fontSize:11, color:'var(--fg-3)', marginTop:2}}>mg7h1v</div>
                </div>
                <span className="badge code">code</span>
                <button className="ih-star outline"><Ic.Star/></button>
              </div>
              <div className="help">Authored fan-in. Combines branches from N parallel <span className="mono">code-mutating</span> upstreams into a single worktree. Acts as an intrinsic barrier — fires only when every branch is <span className="mono">done</span>.</div>
            </div>
            <div className="p-sect">
              <SectionHead title="Inputs" count={1}/>
              <div className="port-row" style={{gridTemplateColumns: '12px 1fr 80px auto'}}>
                <span className="pdot"/>
                <div>
                  <div className="pname">branches <span className="badge" style={{marginLeft: 4, height: 14, padding: '0 4px', fontSize: 9, background:'rgba(245,158,11,0.14)', color:'var(--st-await)', border:'1px solid rgba(245,158,11,0.28)'}}>repeated · fan-in</span></div>
                  <div className="help" style={{marginTop: 2}}>accumulates N parallel edges from upstream <span className="mono">code-mutating</span> nodes</div>
                </div>
                <SidePicker value="left"/>
                <button className="icon-btn" style={{width: 22, height: 22}}><Ic.Kebab/></button>
              </div>
            </div>
            <div className="p-sect">
              <SectionHead title="Outputs" count={1}/>
              <div className="port-row" style={{gridTemplateColumns: '12px 1fr 80px auto'}}>
                <span className="pdot"/>
                <div>
                  <div className="pname">merged</div>
                  <div className="help" style={{marginTop: 2}}>single worktree, frontmatter <span className="mono">{'{branches_merged: int, conflicts: int}'}</span></div>
                </div>
                <SidePicker value="right"/>
                <button className="icon-btn" style={{width: 22, height: 22}}><Ic.Kebab/></button>
              </div>
            </div>
            <div className="p-sect">
              <SectionHead title="Strategy"/>
              <div className="seg" style={{width:'100%', marginTop: 6}}>
                <button className="on" style={{flex: 1}}>three-way</button>
                <button style={{flex: 1}}>squash</button>
                <button style={{flex: 1}}>octopus</button>
              </div>
              <div className="help" style={{marginTop: 6}}>Three-way reuses git's default merge per branch. Conflicts surface as <span className="mono">awaiting_user</span>.</div>
            </div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// 8 ── Canvas · fan-out without Merge · lint info-only banner
function ScreenB8() {
  const planN = { id: 'plan', nid: 'k7m2x9', name: 'Planner', type: 'doc', status: 'pending', x: 100, y: 220, ports: { in:['issue'], out:['plan'] } };
  const a = { id: 'a', nid: 'a1', name: 'Implementer · feature', type: 'code', status: 'pending', x: 400, y: 100, ports: { in:['plan'], out:['diff'] } };
  const b = { id: 'b', nid: 'b1', name: 'Implementer · refactor', type: 'code', status: 'pending', x: 400, y: 340, ports: { in:['plan'], out:['diff'] } };
  const t1 = { id: 't1', nid: 't1', name: 'Tests', type: 'code', status: 'pending', x: 740, y: 100, ports: { in:['diff'], out:['result'] } };
  const t2 = { id: 't2', nid: 't2', name: 'Docs', type: 'doc', status: 'pending', x: 740, y: 340, ports: { in:['diff'], out:['notes'] } };
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="needs-merge" awaiting={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" selectedTemplateId="needs-merge" libraryFocus="templates"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
              <Node node={planN}/>
              <Node node={a}/>
              <Node node={b}/>
              <Node node={t1}/>
              <Node node={t2}/>
              <svg style={{position:'absolute', inset:0, width:'100%', height:'100%', pointerEvents:'none'}}>
                <defs>
                  <marker id="arr3" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L8 4 L0 8 z" fill="#525a68"/></marker>
                  <marker id="arr3w" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto"><path d="M0 0 L8 4 L0 8 z" fill="#f59e0b"/></marker>
                </defs>
                {/* plan -> a (highlighted as fan-out branch a) */}
                <path d="M 300 255 C 350 255, 350 135, 400 135" stroke="#f59e0b" strokeWidth="1.6" fill="none" markerEnd="url(#arr3w)"/>
                <path d="M 300 255 C 350 255, 350 375, 400 375" stroke="#f59e0b" strokeWidth="1.6" fill="none" markerEnd="url(#arr3w)"/>
                {/* a -> t1 */}
                <path d="M 600 135 C 670 135, 670 135, 740 135" stroke="#525a68" strokeWidth="1.5" fill="none" markerEnd="url(#arr3)"/>
                {/* b -> t2 */}
                <path d="M 600 375 C 670 375, 670 375, 740 375" stroke="#525a68" strokeWidth="1.5" fill="none" markerEnd="url(#arr3)"/>
              </svg>
              <div className="fanout-marker" style={{ left: 340, top: 240 }}>
                <span className="fm-dot"/>
                <span>fan-out · 2 code branches</span>
              </div>
            </div>
            <LintBanner x={580} y={500}/>
            <CanvasToolbar activeTool="select"/>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Inspector"/>
          <div className="p-body">
            <div className="empty" style={{padding: '40px 24px'}}>
              <div className="emp-art"><Ic.Cursor/></div>
              <div className="emp-sub" style={{maxWidth: 240}}>Select a node to inspect, or drag a Merge from the Library to silence the lint.</div>
            </div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// 9 ── + New Run modal · pipeline picker fed from starred templates
function ScreenC9() {
  const run = RUNS[0];
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="No run selected" awaiting={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="empty">
              <div className="emp-art"><Ic.Grid/></div>
              <div className="emp-title">Quiet canvas</div>
              <div className="emp-sub">Pick a starred template to launch a new run.</div>
            </div>
            <CanvasToolbar activeTool="select"/>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Detail"/>
          <div className="p-body">
            <div className="empty" style={{padding: '40px 24px'}}><div className="emp-sub" style={{maxWidth: 240}}>Select a node to inspect.</div></div>
          </div>
        </div>
      </Frame>
      <NewRunModalV2 pickerOpen/>
    </div>
  );
}

// 10 ── Spotlight · expanded node terminal
function ScreenS10() {
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" runId="run-a3f">
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={RUNS[0].id}/></div>
        <div className="panel panel-c">{defaultRunCanvas({ run: RUNS[0], sel: 'impl' })}</div>
        <div className="panel panel-r">
          <PanelHead title="Implementer · expanded" actions={<button className="icon-btn"><Ic.Minimize/></button>}/>
          <div className="p-body" style={{display: 'flex', flexDirection: 'column', overflow: 'hidden'}}>
            <div className="exp-pill">
              <span className="st-dot running"/>
              <span style={{fontSize: 12, fontWeight: 600}}>Implementer</span>
              <span className="mono" style={{fontSize: 10.5, color: 'var(--fg-4)'}}>impl · iter 2/5</span>
              <span className="spacer"/>
              <span className="badge code">code</span>
            </div>
            <div style={{flex: 1, padding: '0 10px 10px', display: 'flex', flexDirection: 'column'}}>
              <XTerm expanded session="tmux: pdo/run-a3f/impl · 80×40"
                lines={[
                  { p: 'claude › ', t: 'reading plan.md' },
                  { d: '  ↳ 247 lines, last edited 4 m ago' },
                  { p: 'claude › ', t: 'scanning src/filters/' },
                  { d: '  ↳ 12 files matched · 2 modified' },
                  { tool: 'edit_file', arg: 'src/filters/archived.ts' },
                  { ok: '  ✓ patch applied (+47, -12)' },
                  { tool: 'edit_file', arg: 'src/filters/archived.test.ts' },
                  { ok: '  ✓ patch applied (+82, -0)' },
                  { tool: 'bash', arg: 'pnpm test -- archived.test.ts --watch --runInBand' },
                  { d: '  PASS  src/filters/archived.test.ts' },
                  { d: '    ✓ filters by deletedAt (12 ms)' },
                  { d: '    ✓ excludes parent of archived (8 ms)' },
                  { d: '    ✓ pagination preserved when archived hidden (4 ms)' },
                  { d: '  PASS  src/filters/active.test.ts' },
                  { d: '    ✓ default filter unchanged (3 ms)' },
                  { tool: 'bash', arg: 'pnpm tsc --noEmit' },
                  { ok: '  ✓ typecheck clean' },
                  { p: 'claude › ', t: 'computing diff summary' },
                  { d: '  ↳ 7 files changed · 47 insertions · 12 deletions · 3 tests added' },
                  { tool: 'edit_file', arg: 'artifacts/impl/iter-2/diff.md' },
                  { ok: '  ✓ frontmatter validated against schema' },
                  { p: 'claude › ', t: 'writing diff.md', cursor: true },
                ]}/>
            </div>
            <div className="exp-strip">
              <SectionHead title="Inputs" count={2} collapsed/>
              <SectionHead title="Outputs" count={1} collapsed/>
              <SectionHead title="Initial prompt" collapsed/>
            </div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// 11 ── Spotlight · pipeline info panel · star popover (idle, no Manager terminal)
function ScreenS11() {
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" awaiting={0} activeRuns={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" selectedTemplateId="feature-with-review" libraryFocus="templates"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
              <StartNode x={30} y={215} when="—" runIdSlug="(no run)"/>
              {buildRunNodes().map(n => (
                <Node key={n.id} node={{...n, status: 'pending', iter: undefined}}/>
              ))}
            </div>
            <CanvasToolbar activeTool="select" infoOpen/>
            <MiniMap nodes={buildRunNodes().map(n => ({...n, status: 'pending'}))}/>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Pipeline info" actions={<button className="icon-btn"><Ic.X/></button>}/>
          <PipelineInfoPanel idle starState="diverged" popoverOpen/>
        </div>
      </Frame>
    </div>
  );
}

// 12 ── Spotlight · output schema row · enum-with-allowed editor
function ScreenS12() {
  return (
    <div className="artboard-host" style={{background: 'var(--bg-1)', padding: '60px 80px', display: 'flex', alignItems: 'flex-start', justifyContent: 'center'}}>
      <div style={{width: 520}}>
        <div style={{textAlign: 'center', marginBottom: 24}}>
          <div style={{fontSize: 14, fontWeight: 600, color: 'var(--fg)'}}>Output schema · enum row</div>
          <div style={{fontSize: 11.5, color: 'var(--fg-4)', marginTop: 4, maxWidth: 460, margin: '4px auto 0'}}>
            Per-field type declared inline. Runtime validates the agent's frontmatter at completion; mismatches trigger one in-tmux retry.
          </div>
        </div>
        <div style={{background: 'var(--bg-2)', border: '1px solid var(--line)', borderRadius: 8, boxShadow: '0 12px 40px rgba(0,0,0,0.4)'}}>
          <div style={{padding: '14px 16px', borderBottom: '1px solid var(--line-soft)', display: 'flex', alignItems: 'center', gap: 8}}>
            <span className="pdot"/>
            <div style={{flex: 1}}>
              <div style={{fontSize: 12.5, fontWeight: 600}}>verdict</div>
              <div className="mono" style={{fontSize: 10.5, color: 'var(--fg-4)', marginTop: 1}}>output port · 3 fields</div>
            </div>
            <SidePicker value="right"/>
          </div>
          <div style={{padding: '14px 16px'}}>
            <OutputSchemaEditor enumOpen focusEnum/>
          </div>
        </div>
        <div style={{display:'grid', gridTemplateColumns:'1fr 1fr 1fr', gap: 12, marginTop: 28}}>
          <div className="legend-card">
            <div className="lc-head mono">type</div>
            <div className="lc-body">5 primitives: <span className="mono">string · int · bool · list · enum</span>.</div>
          </div>
          <div className="legend-card">
            <div className="lc-head mono">allowed: […]</div>
            <div className="lc-body">Visible only when type is <span className="mono">enum</span>. Edits propagate to <span className="mono">Switch</span> branches that read the field.</div>
          </div>
          <div className="legend-card">
            <div className="lc-head mono">retry</div>
            <div className="lc-body">On mismatch the agent gets one tmux nudge; second failure surfaces a <span style={{color:'var(--st-failed)'}}>409</span> in NodeDetail.</div>
          </div>
        </div>
      </div>
    </div>
  );
}

window.ScreenA1 = ScreenA1;
window.ScreenA2 = ScreenA2;
window.ScreenA3 = ScreenA3;
window.ScreenA4 = ScreenA4;
window.ScreenB5 = ScreenB5;
window.ScreenB6 = ScreenB6;
window.ScreenB7 = ScreenB7;
window.ScreenB8 = ScreenB8;
window.ScreenC9 = ScreenC9;
window.ScreenS10 = ScreenS10;
window.ScreenS11 = ScreenS11;
window.ScreenS12 = ScreenS12;
