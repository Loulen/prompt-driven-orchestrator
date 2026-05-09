// screens.jsx — Composes the Maestro app shell into screens.
//
// This file has been pruned for the unified-mode iteration:
//   • Edit/Run toggle is removed — no more edit-mode screens (was Screen4, Screen6).
//   • Polled tmux preview is replaced by inline xterm.js — old run screens
//     (Screen1/2/3) and the failed-node screen (Screen12) are dropped pending
//     redraw against the new NodeDetail terminal.
//   • Per-run "Edit this run" tint is removed (was Screen10).
//   • + New Run modal (was Screen7) is dropped pending redraw against
//     the starred-templates dropdown.
//
// Screens that remain as-is per spec ("out of scope"):
//   Screen8  — empty / first-launch state
//   Screen9  — markdown viewer (verdict)
//   Screen9b — markdown viewer (repeated port w/ navigator)
//   Screen13 — Switch inspector
//   Screen14 — Loop inspector
//   Screen15 — Library dropdown (canvas toolbar)
//   Screen16 — Run · Loop + Switch executing
//   Screen17 — Star · favorite states strip

const ART_W = 1440;
const ART_H = 900;

function Frame({ mode = 'run', children, daemon = 'connected', activeRuns = 3, awaiting = 1, breadcrumb, runId, banner, onToggleMode = () => {} }) {
  return (
    <div className={"app-frame maestro" + (mode === 'edit' ? ' edit-mode' : '')}>
      <TopBar mode={mode} onToggleMode={onToggleMode} breadcrumb={breadcrumb} runId={runId}/>
      {banner}
      <div className="shell">{children}</div>
      <StatusBar daemon={daemon} activeRuns={activeRuns} awaiting={awaiting}/>
    </div>
  );
}

// ─────────── Run-mode canvas with nodes / overlay ───────────
// Kept because Screen16 still uses RunOverlay-based composition. New run
// screens for this iteration will compose their own canvases inline.

function RunCanvas({ blockedRun = false, awaitRun = false, runOverride, onSelectNode, selectedNodeId = 'impl', editingRun = false, onToggleEditRun, startWhen = '4 m ago' }) {
  const NODE_OFFSET = 200;
  const baseNodes = FWR_NODES.map(n => ({ ...n, x: n.x + NODE_OFFSET }));
  const nodes = baseNodes.map(n => {
    if (blockedRun) {
      if (n.id === 'plan') return { ...n, status: 'done' };
      if (n.id === 'impl') return { ...n, status: 'failed', iter: '5/5' };
      if (n.id === 'review') return { ...n, status: 'blocked', iter: '5/5' };
      if (n.id === 'tests') return { ...n, status: 'pending' };
      if (n.id === 'merge') return { ...n, status: 'pending' };
    }
    if (awaitRun) {
      if (n.id === 'plan') return { ...n, status: 'done' };
      if (n.id === 'impl') return { ...n, status: 'awaiting_user', iter: '1/5', name: 'Implementer' };
      if (n.id === 'review') return { ...n, status: 'pending' };
    }
    return n;
  });
  const runningSet = new Set(nodes.filter(n => n.status === 'running' || n.status === 'done').map(n => n.id));
  const startX = 30, startY = 215;
  const entryIds = ['plan'];
  const fakeStart = { id: '__start', x: startX - 56, y: startY - 35, status: 'running' };
  const fakeEdges = entryIds.map(t => ({ id: 'se-' + t, from: '__start', to: t }));
  return (
    <div className={"dag-canvas" + (editingRun ? ' run-edit' : '')}>
      <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
        <Edges nodes={[...nodes, fakeStart]} edges={[...FWR_EDGES, ...fakeEdges]} runningSet={new Set([...runningSet, '__start'])}/>
        <HaltIcons nodes={nodes} edges={FWR_EDGES}/>
        <StartNode x={startX} y={startY} when={startWhen} runIdSlug={runOverride.id.slice(-8)}
          selected={selectedNodeId === '__start'}
          onSelect={onSelectNode}
          downstreamRunning/>
        {nodes.map(n => (
          <Node key={n.id} node={n} selected={n.id === selectedNodeId}
            onSelect={onSelectNode}/>
        ))}
        <EdgeLabels nodes={nodes} edges={FWR_EDGES}/>
      </div>
      <RunOverlay run={runOverride} blocked={blockedRun}/>
      <MiniMap nodes={nodes}/>
      <CanvasControls/>
    </div>
  );
}

// ─────────── Edit-mode canvas: kept for Screen13/14/15 (out-of-scope screens) ───────────

function EditCanvas({ selectedNodeId = 'impl', selectedEdgeId = null, onSelectNode = () => {} }) {
  const NODE_OFFSET = 200;
  const nodes = FWR_NODES.map(n => ({ ...n, x: n.x + NODE_OFFSET, status: 'pending', iter: undefined }));
  const runningSet = new Set();
  const startX = 30, startY = 215;
  const endX = 30, endY = 410;
  const fakeStart = { id: '__start', x: startX - 56, y: startY - 35, status: 'pending' };
  const fakeEnd = { id: '__end', x: endX + 28 - 100, y: endY + 56 - 70, status: 'pending' };
  const startEdges = [{ id: 'se-plan', from: '__start', to: 'plan' }];
  const endEdges = [{ id: 'ee-merge', from: 'merge', to: '__end', fromSide: 'bottom', toSide: 'bottom' }];
  return (
    <div className="dag-canvas">
      <div className="dag-inner" style={{ transform: 'translate(20px, 0)' }}>
        <Edges nodes={[...nodes, fakeStart, fakeEnd]} edges={[...FWR_EDGES, ...startEdges, ...endEdges]} runningSet={runningSet}/>
        <HaltIcons nodes={nodes} edges={FWR_EDGES}/>
        <StartNode x={startX} y={startY} when="—" runIdSlug="(no run)" selected={selectedNodeId === '__start'} onSelect={onSelectNode}/>
        <EndNode x={endX} y={endY} reached={false} selected={selectedNodeId === '__end'} onSelect={onSelectNode}/>
        {nodes.map(n => (
          <Node key={n.id} node={n} selected={n.id === selectedNodeId} onSelect={onSelectNode}/>
        ))}
        <EdgeLabels nodes={nodes} edges={FWR_EDGES}/>
      </div>
      <MiniMap nodes={nodes}/>
      <CanvasControls/>
    </div>
  );
}

// ─────────── 8. Empty state — first launch ───────────

function Screen8({ onToggleMode }) {
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="No run selected" onToggleMode={onToggleMode} activeRuns={0} awaiting={0} daemon="connected">
        <div className="panel panel-l">
          <PanelHead title="Runs" count={0}
            actions={<button className="btn primary sm"><Ic.PlusSm/> New Run</button>}/>
          <div className="p-body" style={{display: 'flex'}}>
            <EmptyRuns/>
          </div>
        </div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="empty">
              <div className="emp-art"><Ic.Grid/></div>
              <div className="emp-title">No run selected</div>
              <div className="emp-sub">Select a run or pipeline template from the left panel, or launch a new run to see its DAG.</div>
            </div>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Detail"/>
          <div className="p-body">
            <div className="empty">
              <div className="emp-art"><Ic.Code/></div>
              <div className="emp-sub" style={{maxWidth: 240}}>Select a node in the canvas to see its terminal, inputs and outputs.</div>
            </div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

window.Screen8 = Screen8;

// ─────────── 9. Markdown modal — verdict (single file) ───────────
function Screen9() {
  const run = RUNS[0];
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)}>
        <div className="panel panel-l">
          <RunsListPanel runs={RUNS} selectedId={run.id} onSelect={() => {}}/>
        </div>
        <div className="panel panel-c">
          <RunCanvas runOverride={run} selectedNodeId="review"/>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Reviewer"/>
          <NodeDetail node={FWR_NODES[2]}/>
        </div>
      </Frame>
      <MdVerdictModal open={true} onClose={() => {}}/>
    </div>
  );
}

// ─────────── 9b. Markdown modal — repeated port w/ navigator ───────────
function Screen9b() {
  const run = RUNS[0];
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)}>
        <div className="panel panel-l">
          <RunsListPanel runs={RUNS} selectedId={run.id} onSelect={() => {}}/>
        </div>
        <div className="panel panel-c">
          <RunCanvas runOverride={run} selectedNodeId="impl"/>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <NodeDetail node={FWR_NODES[1]}/>
        </div>
      </Frame>
      <MdFeedbackModal open={true} onClose={() => {}}/>
    </div>
  );
}

// ─────────── 13. Switch node selected — branch editor ───────────
function Screen13() {
  const sw = { id: 'route', nid: 'sw3a9c', name: 'Switch', x: 540, y: 200,
    branches: [
      { name: 'fast_path', summary: 'verdict == "PASS"', conditions: [{field:'verdict', op:'eq', value:'"PASS"'}] },
      { name: 'needs_review', summary: 'verdict in [FAIL, NEEDS_WORK]', conditions: [{field:'verdict', op:'in', value:'[FAIL, NEEDS_WORK]'}] },
      { name: 'else', isDefault: true },
    ]};
  return (
    <div className="artboard-host">
      <Frame mode="edit" breadcrumb="release-flow.yaml">
        <div className="panel panel-l"><PipelinesListPanel pipelines={PIPELINES} selectedId="release-notes" onSelect={()=>{}}/></div>
        <div className="panel panel-c">
          <TabBar tabs={[{id:'release-flow', dirty:true}]} activeId="release-flow"/>
          <div style={{position:'relative', flex:1}}>
            <div className="dag-canvas">
              <div className="dag-inner" style={{transform:'translate(20px,0)'}}>
                <SwitchNode node={sw} selected/>
              </div>
              <EditToolbar/>
              <CanvasControls/>
            </div>
          </div>
        </div>
        <div className="panel panel-r"><PanelHead title="Switch"/><SwitchInspector node={sw} focusedRow={{branch:1, cond:0}}/></div>
      </Frame>
    </div>
  );
}

// ─────────── 14. Loop node selected — config ───────────
function Screen14() {
  const lp = { id: 'review-loop', nid: 'lp7k2x', name: 'Loop', x: 540, y: 200, maxIter: 5 };
  return (
    <div className="artboard-host">
      <Frame mode="edit" breadcrumb="iteration-flow.yaml">
        <div className="panel panel-l"><PipelinesListPanel pipelines={PIPELINES} selectedId="release-notes" onSelect={()=>{}}/></div>
        <div className="panel panel-c">
          <TabBar tabs={[{id:'iteration-flow', dirty:false}]} activeId="iteration-flow" savedAt="2 m ago"/>
          <div style={{position:'relative', flex:1}}>
            <div className="dag-canvas">
              <div className="dag-inner" style={{transform:'translate(20px,0)'}}>
                <LoopNode node={lp} selected/>
              </div>
              <EditToolbar/>
              <CanvasControls/>
            </div>
          </div>
        </div>
        <div className="panel panel-r"><PanelHead title="Loop"/><LoopInspector node={lp}/></div>
      </Frame>
    </div>
  );
}

// ─────────── 15. Library dropdown open ───────────
function Screen15() {
  return (
    <div className="artboard-host">
      <Frame mode="edit" breadcrumb="feature-with-review.yaml">
        <div className="panel panel-l"><PipelinesListPanel pipelines={PIPELINES} selectedId="feature-with-review" onSelect={()=>{}}/></div>
        <div className="panel panel-c">
          <TabBar tabs={[{id:'feature-with-review', dirty:false}]} activeId="feature-with-review"/>
          <div style={{position:'relative', flex:1}}>
            <EditCanvas selectedNodeId={null}/>
            <LibraryDropdown open hoverIndex={1}/>
          </div>
        </div>
        <div className="panel panel-r"><PanelHead title="Inspector"/><div className="p-body"><div className="empty"><div className="emp-sub" style={{maxWidth:240}}>Select a node to inspect.</div></div></div></div>
      </Frame>
    </div>
  );
}

// ─────────── 16. Run with Loop + Switch executing ───────────
function Screen16() {
  const run = { ...RUNS[0], pipeline: 'iteration-flow' };
  const lp = { id: 'lp', nid: 'lp7k2x', name: 'Loop', x: 320, y: 180, maxIter: 5 };
  const sw = { id: 'sw', nid: 'sw3a9c', name: 'Switch', x: 620, y: 180,
    branches: [
      { name: 'pass', summary: 'verdict == "PASS"', conditions:[{field:'verdict', op:'eq', value:'"PASS"'}] },
      { name: 'fail', summary: 'verdict in [FAIL, NEEDS_WORK]', conditions:[{field:'verdict', op:'in', value:'[FAIL, NEEDS_WORK]'}] },
      { name: 'else', isDefault: true },
    ]};
  return (
    <div className="artboard-host">
      <Frame mode="run" breadcrumb="iteration-flow" runId={run.id.slice(-8)}>
        <div className="panel panel-l"><RunsListPanel runs={RUNS} selectedId={run.id} onSelect={()=>{}}/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="dag-inner" style={{transform:'translate(20px,0)'}}>
              <StartNode x={30} y={215} when="4 m ago" runIdSlug={run.id.slice(-8)} downstreamRunning/>
              <LoopNode node={lp} runMode currentIter={3}/>
              <SwitchNode node={sw} activeBranch="fail"/>
            </div>
            <RunOverlay run={run}/>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r"><PanelHead title="Loop"/><LoopInspector node={lp}/></div>
      </Frame>
    </div>
  );
}

// ─────────── 17. Star · library save state — 3-up strip ───────────

function StarSampleCard({ state, label, sub, withPopover = false }) {
  const Icon = state === 'outline' ? Ic.Star : Ic.StarFill;
  return (
    <div style={{
      width: 280, background: 'var(--bg-2)', border: '1px solid var(--line)',
      borderRadius: 8, padding: '14px 14px 16px', display: 'flex', flexDirection: 'column', gap: 12
    }}>
      <div style={{display: 'flex', alignItems: 'center', gap: 8}}>
        <div style={{width: 22, height: 22, borderRadius: 5, background: 'var(--bg-3)',
          display: 'grid', placeItems: 'center', color: 'var(--acc)'}}>
          <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round"><rect x="2" y="2.5" width="9" height="8" rx="1.5"/><path d="M2 6h9"/></svg>
        </div>
        <div style={{flex: 1}}>
          <div style={{fontSize: 12.5, fontWeight: 600, color: 'var(--fg)'}}>Implementer</div>
          <div className="mono" style={{fontSize: 10, color: 'var(--fg-4)', marginTop: 1}}>impl</div>
        </div>
        <span className="badge code">code</span>
        <button className={"ih-star " + state} style={{position:'relative'}}>
          <Icon/>
          {state === 'diverged' && <span className="ih-star-notch"/>}
          {withPopover && state === 'diverged' && (
            <div className="ih-star-pop" style={{position: 'absolute', top: 'calc(100% + 4px)', right: 0}}>
              <div className="ihp-title">Out of sync with library</div>
              <div className="ihp-action">
                <Ic.Spark/> <span><b>Update library entry</b><span className="ihp-sub">Push this node's content to the library.</span></span>
              </div>
              <div className="ihp-action">
                <Ic.Branch/> <span><b>Reset from library</b><span className="ihp-sub">Discard local changes; sync from library.</span></span>
              </div>
              <div className="ihp-action danger">
                <Ic.Trash/> <span><b>Remove from library</b><span className="ihp-sub">Keep this node; delete the library entry.</span></span>
              </div>
            </div>
          )}
        </button>
      </div>
      <div style={{borderTop: '1px solid var(--line)', paddingTop: 10}}>
        <div className="mono" style={{fontSize: 11, color: 'var(--fg-2)', marginBottom: 4}}>{label}</div>
        <div style={{fontSize: 11.5, color: 'var(--fg-3)', lineHeight: 1.5}}>{sub}</div>
      </div>
    </div>
  );
}

function Screen17() {
  return (
    <div className="artboard-host" style={{background:'var(--bg-1)', padding:'40px 24px', display:'flex', flexDirection: 'column', gap: 16}}>
      <div style={{textAlign: 'center', marginBottom: 8}}>
        <div style={{fontSize: 14, fontWeight: 600, color: 'var(--fg)'}}>Library save state</div>
        <div style={{fontSize: 11.5, color: 'var(--fg-4)', marginTop: 4}}>Star at the top-right of the Node Inspector. Indicates whether this node is saved to your library, and if so, whether it's in sync.</div>
      </div>
      <div style={{display: 'flex', gap: 24, justifyContent: 'center', alignItems: 'flex-start', flexWrap: 'wrap', paddingBottom: 200}}>
        <StarSampleCard state="outline"
          label="outline"
          sub={<>No library entry exists for <span className="mono">impl</span>. Click the star to <b style={{color:'var(--fg-2)'}}>save to library</b>.</>}/>
        <StarSampleCard state="synced"
          label="synced"
          sub={<>This node matches a library entry. Tooltip: <i>"In your library — synced"</i>. Click opens a popover with <b style={{color:'var(--fg-2)'}}>Remove from library</b>.</>}/>
        <StarSampleCard state="diverged"
          withPopover
          label="diverged"
          sub={<>Library entry exists but content has drifted. Notch in <span style={{color:'var(--st-blocked)'}}>red</span> at the bottom-right. Popover offers Update / Reset / Remove.</>}/>
      </div>
    </div>
  );
}

window.Screen9 = Screen9;
window.Screen9b = Screen9b;
window.Screen13 = Screen13;
window.Screen14 = Screen14;
window.Screen15 = Screen15;
window.Screen16 = Screen16;
window.Screen17 = Screen17;
window.ART_W = ART_W;
window.ART_H = ART_H;
