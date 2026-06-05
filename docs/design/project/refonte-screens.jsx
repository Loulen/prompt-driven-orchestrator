// refonte-screens.jsx — trimmed deliverable for the canvas-editor refonte.
//   Happy path · build & run a conditional review loop  (4 frames)
//   Component spotlights · only what the happy path doesn't already show
//
// Reuses Frame / TopBar / StatusBar / UnifiedLeftPanel / PanelHead / XTerm /
// EndNode / RunOverlay and the Maestro tokens. New canvas vocab lives in
// refonte-canvas.jsx; detail panels in refonte-inspect.jsx.

// ── Shared review-loop layout (screens 1, 2, 4) ─────────────────────────────
const RL = {
  plan:   { id: 'plan',   x: 240, y: 320, name: 'Planner',     role: 'agent', mark: 'doc',  outs: [{ name: 'plan', side: 'right' }] },
  impl:   { id: 'impl',   x: 460, y: 250, name: 'Implementer', role: 'agent', mark: 'code', outs: [{ name: 'diff', side: 'bottom' }] },
  review: { id: 'review', x: 460, y: 390, name: 'Reviewer',    role: 'agent', mark: 'doc',  outs: [{ name: 'verdict', side: 'right' }] },
};
const RL_REGION = { x: 416, y: 222, w: 252, h: 246 };
const RL_END = { x: 760, y: 322 };
const RL_START = { x: 20, y: 300 };

function reviewLoopEdges({ selFail, selPass } = {}) {
  return [
    { id: 'e1', points: [{x:220,y:328},{x:230,y:328},{x:230,y:343},{x:240,y:343}] },
    { id: 'e2', points: [{x:408,y:343},{x:434,y:343},{x:434,y:273},{x:460,y:273}] },
    { id: 'e3', points: [{x:544,y:296},{x:544,y:390}] },
    { id: 'eFail', points: [{x:628,y:413},{x:652,y:413},{x:652,y:273},{x:628,y:273}], selected: selFail },
    { id: 'ePass', points: [{x:628,y:413},{x:700,y:413},{x:700,y:350},{x:760,y:350}], selected: selPass },
  ];
}

function ReviewLoopWorld({ statuses = {}, counter = '0 / 5', exhausted = false, selectedNode, selFail, selPass, showHints = false, startImages = [], onSelectEdge, onSelectNode }) {
  const node = (k) => ({ ...RL[k], status: statuses[k] || 'pending' });
  const endReached = statuses.__end === 'reached';
  return (
    <>
      <LoopRegion box={RL_REGION} flavor="bounded" name="review-loop" counter={counter}
        exhausted={exhausted} blockText={exhausted ? 'exhausted — unrouted · route from manager' : null}/>
      <RfEdges edges={reviewLoopEdges({ selFail, selPass })}/>
      <RfStart x={RL_START.x} y={RL_START.y} when="6 min ago" runIdSlug="f7c-a31" images={startImages}/>
      <EndNode x={RL_END.x} y={RL_END.y} reached={endReached}/>
      <RfNode node={{ ...node('plan'), outs: [{ name: 'plan', side: 'right' }] }} onSelect={onSelectNode} selected={selectedNode === 'plan'}/>
      <RfNode node={{ ...node('impl'), outs: [{ name: 'diff', side: 'bottom' }] }} onSelect={onSelectNode} selected={selectedNode === 'impl'}/>
      <RfNode node={{ ...node('review'), outs: [{ name: 'verdict', side: 'right', showLabel: showHints }] }} onSelect={onSelectNode} selected={selectedNode === 'review'}/>
      {/* condition pills — multi-match fan-out from the single verdict dot */}
      <CondPill x={652} y={343} when={{ field: 'verdict', op: '=', val: 'FAIL' }} selected={selFail} onSelect={() => onSelectEdge && onSelectEdge('eFail')}/>
      <CondPill x={700} y={381} when={{ field: 'verdict', op: '=', val: 'PASS' }} selected={selPass} onSelect={() => onSelectEdge && onSelectEdge('ePass')}/>
      {/* emergent input hover-names (shown as static examples) */}
      {showHints && <InputLabels labels={[
        { x: 392, y: 256, name: 'plan' },
        { x: 600, y: 256, name: 'review', from: 'reviewer' },
      ]}/>}
    </>
  );
}

// ════════════════════════════════════════════════════════════════════════════
// HAPPY PATH — build & run a conditional review loop
// ════════════════════════════════════════════════════════════════════════════

// 1 · The whole new gestalt in one idle frame.
function RScreen1() {
  return (
    <div className="artboard-host" data-screen-label="01 Canvas — review loop, idle">
      <Frame mode="run" breadcrumb="feature-with-review" activeRuns={2} awaiting={1}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" selectedTemplateId="feature-with-review" libraryFocus="templates"/></div>
        <div className="panel panel-c">
          <RfCanvas miniNodes={[{x:240,y:320,status:'pending'},{x:460,y:250,status:'pending'},{x:460,y:390,status:'pending'},{x:760,y:322,status:'pending'}]}
            worldTransform="translate(150px, 150px)"
            hint={<span>Drag from an <b style={{color:'var(--fg-2)',fontWeight:600}}>output dot</b> to wire · drop an arrow on a node body to make an input · double-click an edge to add a <span className="rh-k">when:</span></span>}>
            <ReviewLoopWorld showHints startImages={['ui-bug.png', 'trace.txt']}
              statuses={{ plan: 'pending', impl: 'pending', review: 'pending' }} counter="max 5"/>
          </RfCanvas>
        </div>
      </Frame>
    </div>
  );
}

// 2 · Edge detail panel — the only surface where when: is authored.
function RScreen2() {
  return (
    <div className="artboard-host" data-screen-label="02 Edge detail — editing a condition">
      <Frame mode="run" breadcrumb="feature-with-review" activeRuns={2} awaiting={1}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="library" selectedTemplateId="feature-with-review" libraryFocus="templates"/></div>
        <div className="panel panel-c">
          <RfCanvas worldTransform="translate(8px, 178px) scale(0.84)"
            miniNodes={[{x:240,y:320,status:'done'},{x:460,y:250,status:'running'},{x:460,y:390,status:'done'},{x:760,y:322,status:'pending'}]}>
            <ReviewLoopWorld selFail counter="2 / 5"
              statuses={{ plan: 'fired', impl: 'running', review: 'fired' }}/>
          </RfCanvas>
        </div>
        <div className="panel panel-r">
          <EdgeDetailPanel
            route={{ from: 'Reviewer', out: 'verdict', to: 'Implementer' }}
            field="is_blocking" fieldType="bool" op="=" value="true" fieldMenuOpen
            region="review-loop"
            trigger={{ fired: true, lastValue: 'true', evaluatedAt: '14:42:06', iter: 2 }}/>
        </div>
      </Frame>
    </div>
  );
}

// 3 · Collection region (single-member badge) + barrier into Merge.
function RScreen3() {
  const start = { x: 20, y: 300 };
  const triage = { id: 'triage', x: 230, y: 322, name: 'Triage', role: 'agent', mark: 'doc', status: 'fired', outs: [{ name: 'issues', side: 'right', showLabel: true }] };
  const fixer  = { id: 'fixer', x: 470, y: 322, name: 'Fixer', role: 'agent', mark: 'code', status: 'running',
                   outs: [{ name: 'patch', side: 'right' }],
                   badge: { flavor: 'collection', glyph: '⇉', text: '5 items' } };
  const merge  = { id: 'merge', x: 720, y: 322, name: 'Merge', role: 'merge', mark: 'code', status: 'pending', outs: [{ name: 'branch', side: 'right' }] };
  const end = { x: 960, y: 322 };
  // barrier: N parallel branches from the fan-out converge into Merge
  const fanEdges = [
    { id: 'f1', points: [{x:638,y:333},{x:672,y:333},{x:672,y:340},{x:720,y:340}] },
    { id: 'f2', points: [{x:638,y:345},{x:720,y:345}] },
    { id: 'f3', points: [{x:638,y:357},{x:672,y:357},{x:672,y:350},{x:720,y:350}] },
  ];
  const edges = [
    { id: 'e0', points: [{x:220,y:328},{x:226,y:328},{x:226,y:345},{x:230,y:345}] },
    { id: 'e1', points: [{x:398,y:345},{x:470,y:345}] },
    ...fanEdges,
    { id: 'e2', points: [{x:888,y:345},{x:960,y:350}] },
  ];
  return (
    <div className="artboard-host" data-screen-label="03 Canvas — collection region + single-member badge">
      <Frame mode="run" breadcrumb="bug-triage" runId="9b2-c04" activeRuns={2} awaiting={1}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={RUNS[2].id}/></div>
        <div className="panel panel-c">
          <RfCanvas worldTransform="translate(120px, 150px)"
            miniNodes={[{x:230,y:322,status:'done'},{x:470,y:322,status:'running'},{x:720,y:322,status:'pending'},{x:960,y:322,status:'pending'}]}
            hint={<span><b style={{color:'var(--p-teal,#c2a15e)',fontWeight:600}}>⇉ collection</b> — one Fixer per issue, fanned in parallel; branches barrier into Merge</span>}>
            <RfEdges edges={edges}/>
            <RfStart x={start.x} y={start.y} when="1 h ago" runIdSlug="9b2-c04"/>
            <EndNode x={end.x} y={end.y}/>
            <RfNode node={triage}/>
            <RfNode node={fixer}/>
            <RfNode node={merge}/>
            <InputLabels labels={[{ x: 648, y: 300, name: 'patch', from: 'fixer[*]' }]}/>
          </RfCanvas>
        </div>
      </Frame>
    </div>
  );
}

// 4 · Running, with an exhausted-unrouted bounded region.
function RScreen4() {
  const run = { id: 'run-2026-05-14-1024-f7c', pipeline: 'feature-with-review', status: 'blocked', when: '12 min ago', elapsed: '12:41' };
  return (
    <div className="artboard-host" data-screen-label="04 Canvas — running · exhausted-unrouted block">
      <Frame mode="run" breadcrumb="feature-with-review" runId="f7c-a31" activeRuns={2} awaiting={1}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={run.id}/></div>
        <div className="panel panel-c">
          <RfCanvas worldTransform="translate(70px, 150px)"
            miniNodes={[{x:240,y:320,status:'done'},{x:460,y:250,status:'done'},{x:460,y:390,status:'running'},{x:760,y:322,status:'pending'}]}
            overlay={<RunOverlay run={run} blocked onOpenManager={() => {}}/>}>
            <ReviewLoopWorld exhausted counter="3 / 3"
              statuses={{ plan: 'fired', impl: 'fired', review: 'running' }}/>
          </RfCanvas>
        </div>
      </Frame>
    </div>
  );
}

// ════════════════════════════════════════════════════════════════════════════
// COMPONENT SPOTLIGHTS — only what the happy path doesn't show
// ════════════════════════════════════════════════════════════════════════════

// 5 · Node card variants (incl. multi-output, Start/End/Merge, all states).
function RScreen5() {
  const Cell = ({ caption, w = 220, h = 150, children }) => (
    <div className="rf-spot-cell">
      <div className="rf-spot-stage" style={{ width: w, height: h }}>{children}</div>
      <div className="rf-spot-caption">{caption}</div>
    </div>
  );
  return (
    <div className="artboard-host" data-screen-label="05 Spotlight — node card variants">
      <div className="rf-spot">
        <div className="rf-spot-title">Node card · variants</div>
        <div className="rf-spot-sub">The slim card carries only role icon, name and the code/doc marker. Multi-output nodes grow one connection dot per produced document; Start, End and Merge keep their distinct silhouettes.</div>
        <div className="rf-spot-grid">
          <Cell caption="code-mutating · doc-only markers">
            <RfNode node={{ id: 'a', x: 26, y: 24, name: 'Implementer', role: 'agent', mark: 'code', status: 'pending', outs: [{ name: 'diff', side: 'right' }] }}/>
            <RfNode node={{ id: 'b', x: 26, y: 86, name: 'Reviewer', role: 'agent', mark: 'doc', status: 'pending', outs: [{ name: 'verdict', side: 'right' }] }}/>
          </Cell>
          <Cell caption="status · pending → running → fired (green) → blocked" w={220} h={262}>
            <RfNode node={{ id: 'p', x: 26, y: 16, name: 'pending', role: 'agent', mark: 'code', status: 'pending', outs: [{ name: 'o', side: 'right' }] }}/>
            <RfNode node={{ id: 'r', x: 26, y: 78, name: 'running', role: 'agent', mark: 'code', status: 'running', outs: [{ name: 'o', side: 'right', produced: false }] }}/>
            <RfNode node={{ id: 'd', x: 26, y: 140, name: 'fired', role: 'agent', mark: 'code', status: 'fired', outs: [{ name: 'o', side: 'right', produced: true }] }}/>
            <RfNode node={{ id: 'bl', x: 26, y: 202, name: 'blocked', role: 'agent', mark: 'doc', status: 'blocked', outs: [{ name: 'o', side: 'right' }] }}/>
          </Cell>
          <Cell caption="multi-output · Debugger emits repro_steps + screenshots — two dots, names on hover" w={240} h={150}>
            <RfNode node={{ id: 'dbg', x: 26, y: 50, name: 'Debugger', role: 'agent', mark: 'code', status: 'fired',
              outs: [{ name: 'repro_steps', side: 'right', t: 0.34, produced: true, showLabel: true }, { name: 'screenshots', side: 'right', t: 0.74, produced: true }] }}/>
          </Cell>
          <Cell caption="Start · End · Merge keep their silhouettes" w={300} h={210}>
            <RfStart x={20} y={16} when="6 min ago" runIdSlug="f7c-a31"/>
            <EndNode x={20} y={140} reached/>
            <RfNode node={{ id: 'mg', x: 96, y: 138, name: 'Merge', role: 'merge', mark: 'code', status: 'pending', w: 150, outs: [{ name: 'branch', side: 'right' }] }}/>
          </Cell>
          <Cell caption="freshly dropped · output dots, no inputs yet (dashed = empty)" w={220} h={150}>
            <RfNode node={{ id: 'fr', x: 26, y: 52, name: 'Tests', role: 'agent', mark: 'code', status: 'pending', outs: [{ name: 'result', side: 'right', empty: true }] }}/>
          </Cell>
        </div>
      </div>
    </div>
  );
}

// 6 · Node inspector — the one place pooling is legible.
function RScreen6() {
  return (
    <div className="artboard-host" data-screen-label="06 Spotlight — node inspector (pooling)">
      <div className="rf-spot">
        <div className="rf-spot-title">Node inspector · pooled inputs spelled out</div>
        <div className="rf-spot-sub">The canvas deliberately hides input pooling. The inspector is where it becomes legible — alongside the node ID, role/prompt editor and output port schemas.</div>
        <div className="rf-spot-grid">
          <div className="rf-spot-cell">
            <div className="rf-spot-panel">
              <NodeInspectorRf
                name="Implementer" nid="9k2x7m" mark="code"
                outputs={[{ name: 'diff', type: 'markdown', fields: [['summary', 'string'], ['files_changed', 'int'], ['verdict', 'enum', 'PASS · FAIL · NEEDS_WORK']] }]}
                inputs={[{ name: 'review', pooled: ['security-reviewer', 'perf-reviewer'] }, { name: 'plan', pooled: ['planner'] }]}/>
            </div>
            <div className="rf-spot-caption">review ← security-reviewer, perf-reviewer — two same-named edges, one logical input.</div>
          </div>
        </div>
      </div>
    </div>
  );
}

// 7 · Region inspector / header — bounded and collection at fidelity.
function RScreen7() {
  return (
    <div className="artboard-host" data-screen-label="07 Spotlight — region inspector / header">
      <div className="rf-spot">
        <div className="rf-spot-title">Loop region · inspector &amp; header</div>
        <div className="rf-spot-sub">Two flavors, two headers. Bounded carries an editable max-iterations bound and a ↻ X/Y counter; collection fans out over a source list field with a ⇉ N-items header and a Merge barrier.</div>
        <div className="rf-spot-grid">
          <div className="rf-spot-cell">
            <div className="rf-spot-stage" style={{ width: 280, height: 96 }}>
              <div className="rf-region bounded" style={{ left: 20, top: 30, width: 240, height: 52 }}>
                <div className="rf-region-head"><span className="rh-glyph">↻</span><span className="rh-count">2 / 5</span><span className="rh-name">review-loop</span></div>
              </div>
            </div>
            <div className="rf-spot-panel"><RegionInspector flavor="bounded"/></div>
            <div className="rf-spot-caption">Bounded — sequential counter, born by auto-detecting a cycle.</div>
          </div>
          <div className="rf-spot-cell">
            <div className="rf-spot-stage" style={{ width: 280, height: 96 }}>
              <div className="rf-region collection" style={{ left: 20, top: 30, width: 240, height: 52 }}>
                <div className="rf-region-head"><span className="rh-glyph">⇉</span><span className="rh-count">5 items</span><span className="rh-name">per-issue</span></div>
              </div>
            </div>
            <div className="rf-spot-panel"><RegionInspector flavor="collection"/></div>
            <div className="rf-spot-caption">Collection — parallel fan-out over a list field.</div>
          </div>
        </div>
      </div>
    </div>
  );
}

// 8 · Edge shaping interaction — affordances no static screen reveals.
function RScreen8() {
  // a manually-pinned edge (with segment handles) and an auto-routed edge
  const pinned = { id: 'p', points: [{x:60,y:70},{x:150,y:70},{x:150,y:150},{x:300,y:150}], selected: true };
  const auto = { id: 'a', points: [{x:60,y:230},{x:180,y:230},{x:180,y:300},{x:300,y:300}] };
  return (
    <div className="artboard-host" data-screen-label="08 Spotlight — edge shaping interaction">
      <div className="rf-spot">
        <div className="rf-spot-title">Edge shaping · manual routing affordances</div>
        <div className="rf-spot-sub">Hovering an edge reveals perpendicular-only segment handles; the first manual drag pins the route. Endpoints re-anchor to any point on a node. A per-edge action resets to automatic right-angle routing.</div>
        <div className="rf-spot-grid">
          <div className="rf-spot-cell">
            <div className="rf-spot-stage" style={{ width: 380, height: 360, position: 'relative' }}>
              {/* source / target node stubs */}
              <RfNode node={{ id: 's1', x: 18, y: 48, w: 120, name: 'Reviewer', role: 'agent', mark: 'doc', status: 'fired', outs: [{ name: 'verdict', side: 'right' }] }}/>
              <RfNode node={{ id: 't1', x: 300, y: 128, w: 120, name: 'Implementer', role: 'agent', mark: 'code', status: 'pending' }}/>
              <RfNode node={{ id: 's2', x: 18, y: 208, w: 120, name: 'Tests', role: 'agent', mark: 'code', status: 'fired', outs: [{ name: 'result', side: 'right' }] }}/>
              <RfNode node={{ id: 't2', x: 300, y: 278, w: 120, name: 'Merge', role: 'merge', mark: 'code', status: 'pending' }}/>
              <RfEdges edges={[{ ...pinned }, { ...auto }]}/>
              {/* segment handles on the pinned edge (perpendicular-only) */}
              <span className="rf-seg-handle h" style={{ left: 150, top: 110 }}/>
              <span className="rf-seg-handle v" style={{ left: 225, top: 150 }}/>
              <span className="rf-endpoint" style={{ left: 300, top: 150 }}/>
              <div className="rf-anno" style={{ left: 150, top: 24 }}><span className="an-key">pinned</span> manual waypoints persisted</div>
              <div className="rf-anno" style={{ left: 110, top: 326 }}><span className="an-key">auto</span> right-angle, re-routes on move</div>
            </div>
            <div className="rf-spot-caption">Pinned edge (top) shows draggable handles + a re-anchorable endpoint. Auto edge (bottom) has the quieter hover affordance.</div>
          </div>
          <div className="rf-spot-cell">
            <div className="rf-spot-panel" style={{ width: 300 }}>
              <div className="rf-edge-head">
                <span className="reh-glyph"><Ic.ArrowRight/></span>
                <div><div className="rf-edge-route"><span className="ern-node">Reviewer</span><span className="ern-mid">.verdict</span></div><div className="rf-edge-sub">manually pinned · 2 waypoints</div></div>
              </div>
              <div className="p-body"><div className="p-sect" style={{ borderBottom: 'none' }}>
                <SectionHead title="Routing"/>
                <div className="rf-else-row" style={{ marginTop: 0 }}>
                  <span style={{ width: 7, height: 7, borderRadius: 3, background: 'var(--acc)', flexShrink: 0 }}/>
                  <div className="er-txt"><div className="er-name">Manually pinned</div><div className="er-help">Route persisted as waypoints; survives node moves.</div></div>
                </div>
                <button className="btn sm" style={{ marginTop: 10, width: '100%', justifyContent: 'center', gap: 6 }}><Ic.Refresh style={{ width: 11, height: 11 }}/> Re-route automatically</button>
              </div></div>
            </div>
            <div className="rf-spot-caption">The reset action lives per-edge in the detail panel.</div>
          </div>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { RScreen1, RScreen2, RScreen3, RScreen4, RScreen5, RScreen6, RScreen7, RScreen8 });
