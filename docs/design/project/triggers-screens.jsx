// triggers-screens.jsx — Triggers iteration.
// Adds a third left-panel tab (Runs | Triggers | Library), trigger rows,
// trigger detail with fire history, the New Run modal's Trigger mode + reject
// state, a session counter in the bottom status bar, and triggered-run
// provenance. Builds on the screens-v2 visual language (Frame/Node/canvas
// unchanged) — only the left panel, right panel content, modal, and status
// bar gain trigger surfaces.

// ─────────────────────────── DATA ───────────────────────────

const TRIGGERS = [
  { id: 'audit-nightly', name: 'Nightly security audit', pipeline: 'security-audit',
    repo: 'acme/web-api', branch: 'main', schedule: 'every day · 09:00', cron: '0 9 * * *',
    enabled: true, outcome: 'success', lastFired: 'today 09:00', next: 'in 18h 04m',
    guard: 'git log --since="24 hours" --oneline | grep -q .', overlap: 'skip' },
  { id: 'cve-sweep', name: 'Dependency CVE sweep', pipeline: 'security-audit',
    repo: 'acme/web-api', branch: 'main', schedule: 'every 15 min', cron: '*/15 * * * *',
    enabled: true, outcome: 'skipped', lastFired: '6 min ago', next: 'in 9m',
    guard: null, overlap: 'skip' },
  { id: 'flaky-triage', name: 'Flaky-test triage', pipeline: 'bug-triage',
    repo: 'acme/web-api', branch: 'main', schedule: 'hourly', cron: '0 * * * *',
    enabled: true, outcome: 'error', lastFired: '12 min ago', next: 'in 48m',
    guard: 'tail -n 200 .ci/last.log', overlap: 'allow' },
  { id: 'docs-release', name: 'Docs refresh on release', pipeline: 'doc-refresh',
    repo: 'acme/docs', branch: 'main', schedule: 'daily · 18:00', cron: '0 18 * * *',
    enabled: false, outcome: 'success', lastFired: '2 d ago', next: 'paused',
    guard: null, overlap: 'skip' },
  { id: 'release-notes', name: 'Weekly release notes', pipeline: 'release-notes',
    repo: 'acme/web-api', branch: 'main', schedule: 'Mondays · 08:00', cron: '0 8 * * 1',
    enabled: true, outcome: 'success', lastFired: '3 d ago', next: 'in 2d 14h',
    guard: null, overlap: 'skip' },
];

const OUTCOME_LABEL = {
  success: 'fired ok', skipped: 'skipped', error: 'guard error', paused: 'inactive',
};
// outcome → existing st-dot class
const OUTCOME_DOT = { success: 'done', skipped: 'archived', error: 'failed', paused: 'pending' };

const AUDIT_FIRE_LOG = [
  { kind: 'fired', time: 'Jun 5 · 09:00', run: 'run-…e7a',
    reason: <>guard exited <span className="mono">0</span> · 3 commits since the last fire — output piped to run input</> },
  { kind: 'skipped', time: 'Jun 4 · 09:00',
    reason: <>previous run <span className="mono">run-…91c</span> still active (started 08:12, audit took 71 min)</> },
  { kind: 'guard_fail', time: 'Jun 3 · 09:00',
    reason: <>guard exited <span className="mono">1</span> — no commits in the last 24h, nothing to audit</> },
  { kind: 'guard_err', time: 'Jun 2 · 09:00',
    reason: <>guard timed out after <span className="mono">30s</span> — treated as no-fire, no run created</> },
  { kind: 'fired', time: 'Jun 1 · 09:00', run: 'run-…b3c',
    reason: <>guard exited <span className="mono">0</span> · 7 commits since the last fire</> },
];

const FIRE_META = {
  fired:      { label: 'fired',      glyph: () => <Ic.Bolt/> },
  skipped:    { label: 'skipped',    glyph: () => <Ic.SkipDot/> },
  guard_fail: { label: 'guard exited non-zero', glyph: () => <Ic.X/> },
  guard_err:  { label: 'guard error / timeout', glyph: () => <Ic.Warn/> },
};

// ─────────────────────── STATUS BAR + FRAME ───────────────────────

function SessionCounter({ active = 3, cap = 8 }) {
  const ratio = active / cap;
  const state = ratio >= 1 ? 'crit' : ratio >= 0.8 ? 'warn' : '';
  return (
    <div className={"sess-counter " + state}>
      <span className="sc-label">sessions</span>
      <span className="sess-gauge"><span className="sg-fill" style={{ width: Math.min(100, ratio * 100) + '%' }}/></span>
      <span className="sc-val">{active}<span className="sc-cap"> / {cap}</span></span>
      {state === 'warn' && <span className="sc-flag">· throttling soon</span>}
      {state === 'crit' && <span className="sc-flag">· at cap · queueing</span>}
    </div>
  );
}

function TrigStatusBar({ daemon = 'connected', activeRuns = 3, awaiting = 1, sessActive = 3, sessCap = 8 }) {
  return (
    <div className="status-bar">
      <div className="item">
        <span className={"dot" + (daemon === 'reconnecting' ? ' warn' : daemon === 'down' ? ' err' : '')}/>
        <span>daemon · {daemon}</span>
      </div>
      <div className="item"><span style={{color:'var(--fg-4)'}}>·</span></div>
      <div className="item">{activeRuns} runs active</div>
      {awaiting > 0 && <div className="item" style={{color:'var(--st-await)'}}>{awaiting} awaiting user</div>}
      <div className="spacer"/>
      <SessionCounter active={sessActive} cap={sessCap}/>
      <div className="item"><span style={{color:'var(--fg-5)'}}>·</span></div>
      <div className="item">v0.4.2-dev</div>
    </div>
  );
}

function TrigFrame({ children, breadcrumb, runId, sessActive = 3, sessCap = 8, activeRuns = 3, awaiting = 1 }) {
  return (
    <div className="app-frame pdo">
      <TopBar mode="run" breadcrumb={breadcrumb} runId={runId}/>
      <div className="shell">{children}</div>
      <TrigStatusBar sessActive={sessActive} sessCap={sessCap} activeRuns={activeRuns} awaiting={awaiting}/>
    </div>
  );
}

// ─────────────────────────── LEFT PANEL ───────────────────────────

function TabStrip({ tab, hasErroring }) {
  const Tab = ({ id, icon, label, count, warn }) => (
    <button className={"lp-tab" + (tab === id ? ' on' : '')}>
      {icon} {label}
      {count != null && <span className={"lp-tab-c" + (warn ? ' dot-warn' : '')}>{count}</span>}
    </button>
  );
  return (
    <div className="lp-tabs">
      <Tab id="runs" icon={<Ic.Play/>} label="Runs" count={RUNS.length}/>
      <Tab id="triggers" icon={<Ic.Clock/>} label="Triggers"
        count={TRIGGERS.filter(t => t.enabled).length} warn={hasErroring}/>
      <Tab id="library" icon={<Ic.Bookmark/>} label="Library"/>
    </div>
  );
}

function TriggerRow({ t, selected }) {
  const dot = t.enabled ? OUTCOME_DOT[t.outcome] : 'pending';
  const resCls = t.outcome === 'success' ? 'tt-ok' : t.outcome === 'skipped' ? 'tt-skip' : 'tt-err';
  return (
    <div className={"trig-row" + (selected ? ' selected' : '') + (t.enabled ? '' : ' disabled')}>
      <div className="trig-dot-wrap">
        <span className={"st-dot " + dot}/>
        <div className="trig-tip">
          <div><span className="tt-k">last run:</span> {t.lastFired}</div>
          <div><span className="tt-k">result:</span> <span className={resCls}>{t.enabled ? OUTCOME_LABEL[t.outcome] : 'disabled'}</span></div>
        </div>
      </div>
      <div className="trig-main">
        <div className="trig-name">{t.name}</div>
        <div className="trig-sub">{t.pipeline}<span className="sep">·</span>{t.repo}</div>
        <div className="trig-schedule">
          <Ic.Repeat/><span className="sch-text">{t.schedule}</span>
        </div>
        <div className="trig-fire">
          {!t.enabled
            ? <span className="tf-paused">disabled · will not fire · last fired {t.lastFired}</span>
            : t.outcome === 'error'
              ? <><span className="tf-err">last fire errored</span> · {t.lastFired} · next {t.next}</>
              : <>next fire <span className="tf-next">{t.next}</span> · last fired {t.lastFired}</>}
        </div>
      </div>
      <div className="trig-right">
        <span className={"toggle" + (t.enabled ? ' on' : '')} title={t.enabled ? 'Disable trigger' : 'Enable trigger'}/>
        <div className="trig-actions">
          <button className="icon-btn" title="Run now"><Ic.Play/></button>
          <button className="icon-btn" title="Edit trigger"><Ic.Pencil/></button>
          <button className="icon-btn danger" title="Delete trigger"><Ic.Trash/></button>
        </div>
      </div>
    </div>
  );
}

function TrigLeftPanel({ tab = 'triggers', selectedTriggerId, selectedRunId, empty = false }) {
  const hasErroring = TRIGGERS.some(t => t.enabled && t.outcome === 'error');
  const headAction =
    tab === 'triggers' ? <button className="btn primary sm"><Ic.PlusSm/> New Trigger</button>
    : tab === 'runs' ? <button className="btn primary sm"><Ic.PlusSm/> New Run</button>
    : <button className="btn sm"><Ic.PlusSm/> New</button>;
  return (
    <>
      <PanelHead title={tab === 'runs' ? 'Runs' : tab === 'triggers' ? 'Triggers' : 'Library'}
        count={tab === 'triggers' && !empty ? TRIGGERS.length : tab === 'runs' ? RUNS.length : null}
        actions={headAction}/>
      <TabStrip tab={tab} hasErroring={hasErroring}/>

      {tab === 'triggers' && !empty && (
        <div className="p-body">
          <div className="trig-list">
            {TRIGGERS.map(t => <TriggerRow key={t.id} t={t} selected={t.id === selectedTriggerId}/>)}
          </div>
        </div>
      )}

      {tab === 'triggers' && empty && (
        <div className="p-body">
          <div className="trig-empty">
            <div className="te-art"><Ic.Clock/><span className="te-bolt"><Ic.Bolt/></span></div>
            <div className="te-title">No triggers yet</div>
            <div className="te-sub">A trigger fires a pipeline run on a schedule — optionally gated by a guard command. Set one up to run audits, sweeps, or reports without launching by hand.</div>
            <button className="btn primary sm"><Ic.PlusSm/> New Trigger</button>
          </div>
        </div>
      )}

      {tab === 'runs' && (
        <>
          <div style={{padding:'8px 12px', display:'flex', gap:6, borderBottom:'1px solid var(--line-soft)'}}>
            {['All','Active','Done','Failed','Archived'].map((f,i) => (
              <button key={f} className="filter-chip" style={i===0 ? {color:'var(--fg)', borderColor:'var(--bg-5)', background:'var(--bg-3)'} : {}}>{f}</button>
            ))}
          </div>
          <div className="p-body">
            <div className="runs-list">
              {RUNS.map(r => (
                <div key={r.id} className={"run-row" + (r.id === selectedRunId ? ' selected' : '')}>
                  <span className={"st-dot " + r.status}/>
                  <div className="rr-main">
                    <div className="rr-name">{r.pipeline}</div>
                    <div className="rr-sub">{r.title}</div>
                    <div className="rr-time">{r.when} · {r.elapsed}</div>
                    {r.trigger && (
                      <span className="prov-badge" title={"Created by trigger · " + r.trigger}>
                        <Ic.Clock/><span className="pb-name">{r.trigger}</span><Ic.ArrowRight/>
                      </span>
                    )}
                  </div>
                  <button className="icon-btn" style={{width:22, height:22}}><Ic.Kebab/></button>
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
              <span className="chev"><Ic.Chevron/></span>
              Pipeline templates<span className="count">· {PIPELINES.length}</span>
              <span className="spacer"/>
              <button className="lib-add"><Ic.PlusSm/></button>
            </div>
            <div className="lib-list">
              {PIPELINES.map(p => (
                <div key={p.id} className="lib-row">
                  <button className="ih-star sm synced"><Ic.StarFill/></button>
                  <div className="lib-row-main">
                    <div className="lib-row-name">{p.id}</div>
                    <div className="lib-row-sub">{p.nodes} nodes · {p.modified}</div>
                  </div>
                  <button className="icon-btn" style={{width:22, height:22}}><Ic.Kebab/></button>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

// ─────────────────────── TRIGGER DETAIL (right) ───────────────────────

function TriggerDetail({ t }) {
  const dot = t.enabled ? OUTCOME_DOT[t.outcome] : 'pending';
  return (
    <>
      <div className="trig-detail-head">
        <span className={"st-dot " + dot} style={{marginTop:5}}/>
        <div className="tdh-main">
          <div className="tdh-name">{t.name}</div>
          <div className="tdh-sub">{t.cron} · {t.id}</div>
        </div>
        <span className={"toggle" + (t.enabled ? ' on' : '')} style={{marginTop:4}}/>
      </div>

      <div className="p-body">
        <div className="p-sect">
          <SectionHead title="Configuration"/>
          <div className="trig-config">
            <span className="tc-k">pipeline</span>
            <span className="tc-v"><span className="tcv-pill"><Ic.Bookmark/> {t.pipeline} · v3</span></span>
            <span className="tc-k">repo</span><span className="tc-v mono">{t.repo}</span>
            <span className="tc-k">branch</span><span className="tc-v mono">{t.branch}</span>
            <span className="tc-k">schedule</span>
            <span className="tc-v">{t.schedule} <span style={{color:'var(--fg-4)', fontFamily:'var(--font-mono)', fontSize:10.5}}>({t.cron})</span></span>
            <span className="tc-k">overlap</span>
            <span className="tc-v">{t.overlap === 'skip' ? 'skip if previous run still active' : 'allow concurrent runs'}</span>
            <span className="tc-k">guard</span>
            <span className="tc-v">
              {t.guard
                ? <code className="tc-guard">{t.guard}</code>
                : <span style={{color:'var(--fg-4)'}}>none · fires on schedule</span>}
            </span>
            <span className="tc-k">input</span>
            <span className="tc-v" style={{color:'var(--fg-3)'}}>
              {t.guard ? "guard's stdout becomes the run input" : 'static input template'}
            </span>
          </div>
        </div>

        <div className="p-sect">
          <SectionHead title="Fire history" count={AUDIT_FIRE_LOG.length}/>
          <div className="fire-log">
            {AUDIT_FIRE_LOG.map((e, i) => {
              const m = FIRE_META[e.kind];
              return (
                <div key={i} className={"fire-entry " + e.kind}>
                  <div className="fire-rail"><span className="fire-glyph">{m.glyph()}</span></div>
                  <div className="fire-body">
                    <div className="fire-top">
                      <span className="fire-label">{m.label}</span>
                      <span className="fire-time">{e.time}</span>
                    </div>
                    <div className="fire-reason">{e.reason}</div>
                    {e.run && <a className="fire-runlink">{e.run} <Ic.ArrowRight/></a>}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </>
  );
}

// ─────────────────────────── CENTER CANVAS ───────────────────────────
// Target pipeline shown read-only, in the refonte hairline node vocabulary.

const SA_PIPE = {
  start: { x: 20, y: 300 },
  scan:   { id: 'scan',   x: 250, y: 305, name: 'Diff scan',  role: 'agent', mark: 'code', outs: [{ name: 'changed', side: 'right' }] },
  audit:  { id: 'audit',  x: 470, y: 305, name: 'CVE audit',  role: 'agent', mark: 'code', outs: [{ name: 'findings', side: 'right' }] },
  report: { id: 'report', x: 690, y: 305, name: 'Report',     role: 'agent', mark: 'doc',  outs: [{ name: 'summary', side: 'right' }] },
  end: { x: 900, y: 305 },
};
const SA_EDGES = [
  { id: 'se', points: [{x:220,y:328},{x:250,y:328}] },
  { id: 'e1', points: [{x:418,y:328},{x:470,y:328}] },
  { id: 'e2', points: [{x:638,y:328},{x:690,y:328}] },
  { id: 'e3', points: [{x:858,y:328},{x:900,y:333}] },
];

function TrigTargetCanvas({ statuses = {}, hint, running = false }) {
  const st = (k) => statuses[k] || 'pending';
  const mini = [
    { x: 250, y: 305, status: st('scan') }, { x: 470, y: 305, status: st('audit') },
    { x: 690, y: 305, status: st('report') }, { x: 900, y: 305, status: 'pending' },
  ];
  return (
    <RfCanvas worldTransform="translate(24px, 150px) scale(0.84)" miniNodes={mini} hint={hint}>
      <RfEdges edges={running
        ? SA_EDGES.map(e => (e.id === 'se' || e.id === 'e1') ? { ...e, accent: true } : e)
        : SA_EDGES}/>
      <RfStart x={SA_PIPE.start.x} y={SA_PIPE.start.y} when="today 09:00" runIdSlug={running ? '…e7a' : '(no run)'}/>
      <EndNode x={SA_PIPE.end.x} y={SA_PIPE.end.y}/>
      <RfNode node={{ ...SA_PIPE.scan,   status: st('scan'),   outs: [{ name: 'changed', side: 'right', produced: st('scan') === 'fired' }] }}/>
      <RfNode node={{ ...SA_PIPE.audit,  status: st('audit'),  outs: [{ name: 'findings', side: 'right', produced: st('audit') === 'fired' }] }}/>
      <RfNode node={{ ...SA_PIPE.report, status: st('report'), outs: [{ name: 'summary', side: 'right' }] }}/>
    </RfCanvas>
  );
}

function trigHint(name, body) {
  return <span><b style={{color:'var(--fg-2)', fontWeight:600}}>{name}</b> · {body}</span>;
}

// ─────────────────── NEW RUN MODAL — TRIGGER MODE ───────────────────

function ModalSharedFields({ rejectInput = false }) {
  return (
    <>
      <div className="field">
        <label>Pipeline template</label>
        <div className="picker">
          <button className="picker-btn">
            <Ic.StarFill style={{color:'var(--acc)'}}/>
            <span className="mono" style={{color:'var(--fg)'}}>feature-with-review</span>
            <span className="mono" style={{color:'var(--fg-4)', fontSize:10.5}}>· 5 nodes · prompt required</span>
            <span className="spacer"/>
            <Ic.Chevron style={{transform:'rotate(-90deg)'}}/>
          </button>
        </div>
      </div>
      <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:12}}>
        <div className="field" style={{marginBottom:0}}>
          <label>Target repo</label>
          <div className="input mono" style={{display:'flex', alignItems:'center'}}>acme/web-api</div>
        </div>
        <div className="field" style={{marginBottom:0}}>
          <label>Source branch</label>
          <div className="input mono" style={{display:'flex', alignItems:'center'}}>main</div>
        </div>
      </div>
      <div className="field" style={{marginBottom:0}}>
        <label>Input template <span className="relabel">· optional</span></label>
        <textarea className="textarea mono" style={rejectInput ? {minHeight:64, borderColor:'rgba(168,120,74,0.4)'} : {minHeight:64}}
          defaultValue={rejectInput ? '' : 'Audit changed files for new dependency CVEs and unsafe patterns. Summarise findings by severity.'}
          placeholder="Used as the run input when there is no guard…"/>
        <div className="help">Used as the run input when there's no guard. Required here because <span className="mono" style={{color:'var(--fg-3)'}}>feature-with-review</span> sets <span className="mono" style={{color:'var(--fg-3)'}}>prompt_required: true</span>.</div>
      </div>
    </>
  );
}

function NewRunModalTrigger({ reject = false }) {
  return (
    <div className="modal-bg">
      <div className="modal" style={{width: 540, maxHeight: '92%', display:'flex', flexDirection:'column'}}>
        <div className="modal-head">
          <h2>New Run</h2>
          <button className="icon-btn"><Ic.X/></button>
        </div>
        <div className="modal-body" style={{overflow:'auto'}}>
          <div className="mode-toggle">
            <button><Ic.Play/> Run now</button>
            <button className="on trig"><Ic.Clock/> Trigger</button>
          </div>
          <div className="mode-caption">A trigger doesn't run now — it fires this pipeline on a schedule, optionally gated by a guard command.</div>

          <ModalSharedFields rejectInput={reject}/>

          <div className="trig-group">
            <div className="tg-head"><Ic.Clock/> Trigger settings</div>
            <div className="tg-body">
              <div className="field" style={{marginBottom:0}}>
                <label>Name</label>
                <input className="input" defaultValue={reject ? '' : 'Nightly CVE audit'} placeholder="e.g. Nightly security audit"/>
              </div>

              <div className="field" style={{marginBottom:0}}>
                <label>Schedule</label>
                <div className="sched-presets">
                  <button className="sched-chip">every 15 min</button>
                  <button className="sched-chip">hourly</button>
                  <button className="sched-chip on">daily<span className="sc-time">09:00</span></button>
                  <button className="sched-chip">weekly</button>
                </div>
                <div className="sched-time-row">
                  <span className="stl">at</span>
                  <input className="input mono" defaultValue="09:00"/>
                  <span className="stl" style={{color:'var(--fg-4)'}}>local time</span>
                </div>
                <div className="cron-hatch">
                  <button className="cron-toggle open"><span className="chev"><Ic.Chevron/></span> Raw cron expression</button>
                  <div className="cron-field">
                    <input className="input mono" defaultValue="0 9 * * *"/>
                    <div className="cron-resolved"><span className="ok">✓</span> resolves to “every day at 09:00” · next: tomorrow 09:00</div>
                  </div>
                </div>
              </div>

              <div className="field" style={{marginBottom:0}}>
                <label>Guard command <span style={{color:'var(--fg-4)', fontWeight:400}}>· optional</span></label>
                <input className="input mono" defaultValue={reject ? '' : 'git log --since="24 hours" --oneline | grep -q .'}
                  placeholder="single shell command…"/>
                <div className="guard-help">
                  Runs before each fire. Exit <span className="g-exit0">0</span> fires the run; a <span className="g-exitn">non-zero</span> exit skips it. The command's <span className="mono">stdout</span> becomes the run's input.
                </div>
              </div>

              <div className="field" style={{marginBottom:0}}>
                <label>Overlap</label>
                <div className="overlap-choice">
                  <div className="overlap-opt on">
                    <span className="overlap-radio"/>
                    <div>
                      <div className="oo-name">Skip if a previous run is still active<span className="oo-def">default</span></div>
                      <div className="oo-sub">A scheduled fire is dropped (logged as skipped) while the last run is still going.</div>
                    </div>
                  </div>
                  <div className="overlap-opt">
                    <span className="overlap-radio"/>
                    <div>
                      <div className="oo-name">Allow concurrent runs</div>
                      <div className="oo-sub">Each fire starts a run regardless — counts against the session cap.</div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div className={"modal-foot" + (reject ? ' with-reason' : '')}>
          {reject && (
            <div className="reject-reason">
              <Ic.Warn/>
              <div>This pipeline needs a prompt — add a guard, an input template, or mark the pipeline as not requiring a prompt. <span className="mono">prompt_required: true</span></div>
            </div>
          )}
          <div className="foot-actions">
            <button className="btn">Cancel</button>
            <button className={"btn primary" + (reject ? '' : '')} disabled={reject}><Ic.Clock/> Create trigger</button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────── SCREENS ───────────────────────────

// A1 — Triggers tab, populated + session counter (normal)
function T_A1() {
  return (
    <div className="artboard-host">
      <TrigFrame breadcrumb="Triggers" sessActive={3} sessCap={8} activeRuns={3} awaiting={1}>
        <div className="panel panel-l"><TrigLeftPanel tab="triggers" selectedTriggerId="audit-nightly"/></div>
        <div className="panel panel-c">
          <TrigTargetCanvas hint={trigHint('Nightly security audit', 'target pipeline (read-only) · select a trigger to inspect its config & fire history →')}/>
        </div>
        <div className="panel panel-r"><TriggerDetail t={TRIGGERS[0]}/></div>
      </TrigFrame>
    </div>
  );
}

// A2 — Triggers tab, empty
function T_A2() {
  return (
    <div className="artboard-host">
      <TrigFrame breadcrumb="Triggers" sessActive={3} sessCap={8} activeRuns={3} awaiting={1}>
        <div className="panel panel-l"><TrigLeftPanel tab="triggers" empty/></div>
        <div className="panel panel-c">
          <TrigTargetCanvas hint={trigHint('No trigger selected', 'create a trigger to fire a pipeline on a schedule, without launching by hand')}/>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Trigger"/>
          <div className="empty" style={{padding:'40px 24px'}}>
            <div className="emp-art"><Ic.Clock/></div>
            <div className="emp-title">Nothing selected</div>
            <div className="emp-sub">A trigger's configuration and fire history show here once you select or create one.</div>
          </div>
        </div>
      </TrigFrame>
    </div>
  );
}

// A3 — Trigger detail, fire history (debug surface) — select the audit trigger
function T_A3() {
  return (
    <div className="artboard-host">
      <TrigFrame breadcrumb="Triggers" runId="audit-nightly" sessActive={4} sessCap={8} activeRuns={4} awaiting={1}>
        <div className="panel panel-l"><TrigLeftPanel tab="triggers" selectedTriggerId="audit-nightly"/></div>
        <div className="panel panel-c">
          <TrigTargetCanvas hint={trigHint('Nightly security audit', 'security-audit · acme/web-api · fires daily 09:00 when the guard passes')}/>
        </div>
        <div className="panel panel-r"><TriggerDetail t={TRIGGERS[0]}/></div>
      </TrigFrame>
    </div>
  );
}

// A4 — Runs tab, triggered-run provenance + session counter near cap (warning)
function T_A4() {
  return (
    <div className="artboard-host">
      <TrigFrame breadcrumb="Runs" sessActive={7} sessCap={8} activeRuns={7} awaiting={1}>
        <div className="panel panel-l"><TrigLeftPanel tab="runs" selectedRunId={RUNS[5].id}/></div>
        <div className="panel panel-c">
          <TrigTargetCanvas running statuses={{ scan: 'fired', audit: 'running', report: 'pending' }}
            hint={trigHint('Run · security-audit', 'created by the Nightly security audit trigger — see the provenance badge in the list')}/>
        </div>
        <div className="panel panel-r">
          <div className="trig-detail-head">
            <span className="st-dot failed" style={{marginTop:5}}/>
            <div className="tdh-main">
              <div className="tdh-name">security-audit</div>
              <div className="tdh-sub">{RUNS[5].id.slice(-12)} · v3 · failed</div>
            </div>
          </div>
          <div className="p-body">
            <div className="p-sect">
              <SectionHead title="Provenance"/>
              <div className="trig-config">
                <span className="tc-k">source</span>
                <span className="tc-v"><span className="tcv-pill"><Ic.Clock/> trigger · Nightly security audit</span></span>
                <span className="tc-k">fired</span><span className="tc-v mono">21 h ago · scheduled 09:00</span>
                <span className="tc-k">guard</span><span className="tc-v">exited <span className="mono" style={{color:'var(--st-done)'}}>0</span> · 5 commits</span>
                <span className="tc-k">input</span><span className="tc-v" style={{color:'var(--fg-3)'}}>from guard stdout</span>
              </div>
              <a className="fire-runlink" style={{marginTop:10}}><Ic.Clock/> open trigger <Ic.ArrowRight/></a>
              <div className="help" style={{marginTop:10, lineHeight:1.55}}>
                Manually-launched runs carry no such badge — provenance is shown only on rows a trigger created.
              </div>
            </div>
            <div className="p-sect">
              <SectionHead title="Sessions"/>
              <div className="help" style={{lineHeight:1.6}}>
                7 of 8 agent sessions active — the bottom status-bar gauge has shifted to its <span style={{color:'var(--st-await)'}}>warning</span> treatment. Scheduled fires that would exceed the cap queue until a session frees up.
              </div>
            </div>
          </div>
        </div>
      </TrigFrame>
    </div>
  );
}

// B1 — New Run modal, Trigger mode, populated
function T_B1() {
  return (
    <div className="artboard-host">
      <TrigFrame breadcrumb="feature-with-review" sessActive={3} sessCap={8}>
        <div className="panel panel-l"><TrigLeftPanel tab="triggers" selectedTriggerId="audit-nightly"/></div>
        <div className="panel panel-c"><TrigTargetCanvas hint={trigHint('feature-with-review', 'configuring a new trigger for this pipeline')}/></div>
        <div className="panel panel-r"><TriggerDetail t={TRIGGERS[0]}/></div>
        <NewRunModalTrigger/>
      </TrigFrame>
    </div>
  );
}

// B2 — New Run modal, Trigger mode, reject (prompt-required, no guard, empty input)
function T_B2() {
  return (
    <div className="artboard-host">
      <TrigFrame breadcrumb="feature-with-review" sessActive={3} sessCap={8}>
        <div className="panel panel-l"><TrigLeftPanel tab="triggers" selectedTriggerId="audit-nightly"/></div>
        <div className="panel panel-c"><TrigTargetCanvas hint={trigHint('feature-with-review', 'configuring a new trigger for this pipeline')}/></div>
        <div className="panel panel-r"><TriggerDetail t={TRIGGERS[0]}/></div>
        <NewRunModalTrigger reject/>
      </TrigFrame>
    </div>
  );
}

// ─────────────────────────── SPOTLIGHTS ───────────────────────────

function SpotLabel({ children }) { return <div className="legend-card"><div className="lc-body">{children}</div></div>; }

// Session counter — three states, the bottom-bar gauge
function T_SpotSessions() {
  const states = [
    { a: 3, c: 8, note: 'Normal · plenty of headroom. Reads neutral.' },
    { a: 7, c: 8, note: 'Near cap · warning treatment. The user sees throttling coming.' },
    { a: 8, c: 8, note: 'At cap · queueing. New fires wait for a session to free up.' },
  ];
  return (
    <div className="artboard-host" style={{background:'var(--bg-1)', padding:28, overflow:'auto'}}>
      <div style={{maxWidth:760, margin:'0 auto', display:'flex', flexDirection:'column', gap:18}}>
        <div>
          <div style={{fontSize:13, fontWeight:600, color:'var(--fg)'}}>Session counter — bottom status bar</div>
          <div style={{fontSize:11.5, color:'var(--fg-3)', marginTop:4}}>A quiet gauge, not a focal point. It shifts treatment as active sessions approach the configured cap.</div>
        </div>
        {states.map((s, i) => (
          <div key={i}>
            <div style={{border:'1px solid var(--line)', borderRadius:8, overflow:'hidden', background:'var(--bg-2)'}}>
              <div className="status-bar" style={{height:26}}>
                <div className="item"><span className="dot"/><span>daemon · connected</span></div>
                <div className="item"><span style={{color:'var(--fg-4)'}}>·</span></div>
                <div className="item">{s.a} runs active</div>
                <div className="spacer"/>
                <SessionCounter active={s.a} cap={s.c}/>
                <div className="item"><span style={{color:'var(--fg-5)'}}>·</span></div>
                <div className="item">v0.4.2-dev</div>
              </div>
            </div>
            <div style={{fontSize:11, color:'var(--fg-4)', marginTop:6, fontFamily:'var(--font-mono)'}}>{s.note}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

// Trigger row anatomy — outcomes, toggle, hover actions
function T_SpotRow() {
  const sample = [
    { ...TRIGGERS[0], _note: 'success · enabled — gold dot, next-fire in accent' },
    { ...TRIGGERS[1], _note: 'skipped — grey dot (last fire skipped on overlap)' },
    { ...TRIGGERS[2], _note: 'error — amber dot, "last fire errored" line' },
    { ...TRIGGERS[3], _note: 'disabled — dimmed, toggle off, "will not fire"' },
  ];
  // re-map sample[2] to flaky (error) for clarity
  sample[2] = { ...TRIGGERS[2], _note: 'error — amber dot, "last fire errored" line' };
  return (
    <div className="artboard-host" style={{background:'var(--bg-1)', padding:28, overflow:'auto'}}>
      <div style={{maxWidth:680, margin:'0 auto', display:'flex', flexDirection:'column', gap:16}}>
        <div>
          <div style={{fontSize:13, fontWeight:600, color:'var(--fg)'}}>Trigger row — state &amp; identity first</div>
          <div style={{fontSize:11.5, color:'var(--fg-3)', marginTop:4}}>Status dot (hover for last-run tooltip) + name lead. Schedule and next/last fire support. Toggle is always visible; run-now / edit / delete stay quiet until hover.</div>
        </div>
        <div style={{background:'var(--bg-2)', border:'1px solid var(--line)', borderRadius:8, padding:6}}>
          <div className="trig-list">
            {sample.map((t, i) => (
              <React.Fragment key={i}>
                <TriggerRow t={t} selected={i === 0}/>
                <div style={{fontSize:10, color:'var(--fg-4)', fontFamily:'var(--font-mono)', padding:'0 10px 8px'}}>{t._note}</div>
              </React.Fragment>
            ))}
          </div>
        </div>
        <SpotLabel>Hover any row to reveal run-now · edit · delete. Hover the status dot for “last run … / result …”.</SpotLabel>
      </div>
    </div>
  );
}

// Schedule chooser + guard contract (the two trigger-only inputs that carry contracts)
function T_SpotSchedGuard() {
  return (
    <div className="artboard-host" style={{background:'var(--bg-1)', padding:28, overflow:'auto'}}>
      <div style={{maxWidth:520, margin:'0 auto', display:'flex', flexDirection:'column', gap:18}}>
        <div>
          <div style={{fontSize:13, fontWeight:600, color:'var(--fg)'}}>Schedule chooser &amp; Guard contract</div>
          <div style={{fontSize:11.5, color:'var(--fg-3)', marginTop:4}}>Presets cover the common cases; the raw cron hatch is always one click away and shows a plain-English resolution.</div>
        </div>
        <div className="trig-group">
          <div className="tg-head"><Ic.Clock/> Schedule</div>
          <div className="tg-body">
            <div className="field" style={{marginBottom:0}}>
              <div className="sched-presets">
                <button className="sched-chip">every 15 min</button>
                <button className="sched-chip">hourly</button>
                <button className="sched-chip on">daily<span className="sc-time">09:00</span></button>
                <button className="sched-chip">weekly</button>
              </div>
              <div className="cron-hatch">
                <button className="cron-toggle open"><span className="chev"><Ic.Chevron/></span> Raw cron expression</button>
                <div className="cron-field">
                  <input className="input mono" defaultValue="0 9 * * *"/>
                  <div className="cron-resolved"><span className="ok">✓</span> resolves to “every day at 09:00”</div>
                </div>
              </div>
            </div>
          </div>
        </div>
        <div className="trig-group">
          <div className="tg-head"><Ic.Shield/> Guard command</div>
          <div className="tg-body">
            <div className="field" style={{marginBottom:0}}>
              <input className="input mono" defaultValue='git log --since="24 hours" --oneline | grep -q .'/>
              <div className="guard-help">
                Runs before each fire. Exit <span className="g-exit0">0</span> fires the run; a <span className="g-exitn">non-zero</span> exit skips it. The command's <span className="mono">stdout</span> becomes the run's input.
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

window.T_A1 = T_A1; window.T_A2 = T_A2; window.T_A3 = T_A3; window.T_A4 = T_A4;
window.T_B1 = T_B1; window.T_B2 = T_B2;
window.T_SpotSessions = T_SpotSessions; window.T_SpotRow = T_SpotRow; window.T_SpotSchedGuard = T_SpotSchedGuard;
