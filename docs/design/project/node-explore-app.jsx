// node-explore-app.jsx — three-sheet exploration document
// Sheet 1: Node status × hypothesis matrix + specials strip per hypothesis
// Sheet 2: Port row hypotheses, 6-cell rows
// Sheet 3: Selection cohabitation with status, per hypothesis

const HYPOTHESES = [
  { id: 'h1', name: 'Solid',         cap: 'Flat 1.5px conventional stroke. Quiet, reads as a tag.' },
  { id: 'h2', name: 'Glow',          cap: 'Thin stroke + outer atmospheric glow. Soft, painterly.' },
  { id: 'h3', name: 'Breathing pulse', cap: 'Cadre + outer ring that breathes only when running.' },
  { id: 'h4', name: 'Gap-offset ring', cap: 'Outer ring 3px outside the card. Crisp, poster-like.' },
  { id: 'h5', name: 'Marching dash', cap: 'Dashed cadre; dashes march for running, hold for others.' },
];

const STATUS_ROWS = [
  { id: 'pending',       name: 'Pending',       cap: 'No cadre. Absence is the signal.' },
  { id: 'running',       name: 'Running',       cap: 'Cadre in --st-running. Optional motion.' },
  { id: 'awaiting_user', name: 'Awaiting user', cap: 'Cadre in --st-await. Stalled, distinct.' },
  { id: 'completed',     name: 'Completed',     cap: 'Cadre in --st-done. Static.' },
  { id: 'failed',        name: 'Failed',        cap: 'Cadre + dominant overlay. The only "shout".' },
  { id: 'sel-running',   name: 'Selected + running',
    cap: 'Status cadre stays readable through the --acc selection signal.' },
];

function Cell({ children, tag }){
  return (
    <div className="cell">
      {tag && <span className="ed-tag mono">{tag}</span>}
      {children}
    </div>
  );
}

/* ---------------- Sheet 1 ---------------- */

function Sheet1(){
  return (
    <section className="sheet" id="sheet-1">
      <div className="sheet-head">
        <span className="sheet-num mono">SHEET 01</span>
        <h2 className="sheet-title">Node card — status × treatment matrix</h2>
      </div>
      <p className="sheet-desc">
        Same generic <span className="mono" style={{color:'var(--fg-2)'}}>code-mutating</span> node card, repeated across
        five hypotheses (columns) and six status rows. Pending shows the absence of a cadre as the signal.
        The selected row stacks an <span className="mono" style={{color:'var(--acc)'}}>--acc</span> outer ring
        outside the status cadre so both channels read at once. Failed states stack a dominant overlay
        (badge + optional tint) on top of the cadre.
      </p>

      <div className="legend">
        <span className="lk"><span className="sw"/>pending — no cadre</span>
        <span className="lk"><span className="sw running"/>running — st-running</span>
        <span className="lk"><span className="sw await"/>awaiting — st-await</span>
        <span className="lk"><span className="sw done"/>completed — st-done</span>
        <span className="lk"><span className="sw failed"/>failed — st-failed + overlay</span>
        <span className="lk"><span className="sw sel"/>selected — --acc, outer channel</span>
      </div>

      <div className="matrix">
        <div className="corner"/>
        {HYPOTHESES.map((h, i) => (
          <div key={h.id} className="col-head">
            <span className="col-tag mono">{`H${i+1}`}</span>
            <span className="col-name">{h.name}</span>
            <span className="col-cap">{h.cap}</span>
          </div>
        ))}

        {STATUS_ROWS.map((row) => (
          <React.Fragment key={row.id}>
            <div className="row-head">
              <span className="row-tag mono">{row.id}</span>
              <span className="row-name">{row.name}</span>
              <span className="row-cap">{row.cap}</span>
            </div>
            {HYPOTHESES.map((h) => (
              <div key={h.id + '-' + row.id} className={"cell " + h.id}>
                <NodeCard
                  status={row.id === 'sel-running' ? 'running' : row.id}
                  selected={row.id === 'sel-running'}
                  kind="code"
                  name="rewrite_section"
                  nid="nd_4f2a"
                  ports={{ in: ['in'], out: ['out'] }}
                  hypothesisPort="p2"
                  failOverlay={h.id === 'h2' || h.id === 'h4' ? 'badge+tint' : 'badge'}
                />
              </div>
            ))}
          </React.Fragment>
        ))}
      </div>

      {/* Specials strip per hypothesis */}
      {HYPOTHESES.map((h, i) => (
        <div key={'strip-' + h.id} className="specials-strip">
          <div className="strip-head">
            <span className="h-tag mono">{`H${i+1} · ${h.name}`}</span>
            <h4>Across special nodes</h4>
            <span className="strip-cap">Switch · Loop with unconventional sides · Merge · Start + End with per-port indicator</span>
          </div>
          <div className={"specials-grid " + h.id}>
            <div className="cell">
              <NodeCard status="running" kind="switch" name="route" nid="nd_sw1"
                        ports={{ in: ['in'], out: ['pass','retry','default'] }}
                        portSides={{ in: 'left', pass: 'right', retry: 'right', default: 'right' }}
                        hypothesisPort="p2"/>
              <div className="spec-cap">SWITCH · 1 in / 3 out</div>
            </div>
            <div className="cell">
              <NodeCard status="running" kind="loop" name="iterate" nid="nd_lp1"
                        ports={{ in: ['in','break'], out: ['body','done'] }}
                        portSides={{ in: 'left', break: 'top', body: 'right', done: 'bottom' }}
                        hypothesisPort="p2"/>
              <div className="spec-cap">LOOP · break ↑ · done ↓</div>
            </div>
            <div className="cell">
              <NodeCard status="completed" kind="merge" name="merge" nid="nd_mg1"
                        ports={{ in: ['branches'], out: ['merged'] }}
                        portSides={{ branches:'left', merged:'right' }}
                        hypothesisPort="p2"/>
              <div className="spec-cap">MERGE · 1 in / 1 out</div>
            </div>
            <div className="cell">
              <NodeCard status="completed" kind="start" name="Start" nid=""
                        ports={{ in: [], out: ['out'] }}
                        portSides={{ out:'right' }}
                        hypothesisPort="p2"/>
              <div className="spec-cap">START · always-completed</div>
            </div>
            <div className="cell">
              <NodeCard status="running" kind="end" name="End" nid=""
                        ports={{ in: ['a','b','c'], out: [] }}
                        portSides={{ a:'left', b:'top', c:'bottom' }}
                        portStates={{ a:{indicator:'ok'}, b:{indicator:'ok'}, c:{indicator:'pending'} }}
                        hypothesisPort="p2"/>
              <div className="spec-cap">END · aggregate cadre + per-port</div>
            </div>
          </div>
        </div>
      ))}
    </section>
  );
}

/* ---------------- Sheet 2 ---------------- */

const PORT_HYPS = [
  { id: 'p1', name: 'Chevron tab',  cap: 'Tiny geometric tab pokes outward; label sits inside the card.' },
  { id: 'p2', name: 'Capsule pill', cap: 'Full pill (chevron + label) straddles the card edge.' },
  { id: 'p3', name: 'Inset row',    cap: 'Row lives inside the card; chevron flush to the edge marks direction.' },
  { id: 'p4', name: 'Bare label',   cap: 'No pill; just label and a 2px edge tick that doubles as drop target.' },
  { id: 'p5', name: 'Notch chip',   cap: 'Small chip half-buried in the card edge, like a label tab.' },
];

const PORT_COLS = [
  { tag: '01', sub: 'right · output',    side: 'right',  dir: 'out', label: 'body',   drop: false, debug: false },
  { tag: '02', sub: 'left · input',      side: 'left',   dir: 'in',  label: 'in',     drop: false, debug: false },
  { tag: '03', sub: 'top · input',       side: 'top',    dir: 'in',  label: 'break',  drop: false, debug: false },
  { tag: '04', sub: 'bottom · output',   side: 'bottom', dir: 'out', label: 'done',   drop: false, debug: false },
  { tag: '05', sub: 'drop target · hover', side: 'left', dir: 'in',  label: 'in',     drop: true,  debug: false, drag: true },
  { tag: '06', sub: 'debug · halo',      side: 'right',  dir: 'out', label: 'body',   drop: false, debug: true },
];

function PortRowCell({ col, hypId }){
  const isVertical = col.side === 'top' || col.side === 'bottom';
  // Compact card so the port is dominant. For top/bottom orientations we
  // give the card a bit more height so the port doesn't crowd content.
  return (
    <div className="port-cell">
      <span className="ed-tag mono">{col.tag}</span>
      <div className={hypId}>
        <NodeCard compact
          status="pending" kind="code"
          name="rewrite_section" nid=""
          ports={col.dir === 'in' ? { in: [col.label] } : { out: [col.label] }}
          portSides={{ [col.label]: col.side }}
          portStates={{ [col.label]: { drop: col.drop } }}
          hypothesisPort={hypId}
          debug={col.debug}/>
      </div>
      {col.drag && (
        <svg style={{position:'absolute', left:0, top:0, width:'100%', height:'100%', pointerEvents:'none'}}>
          <defs>
            <marker id={"arr-drag-"+hypId} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto">
              <path d="M0 0 L8 4 L0 8 z" fill="#10b981"/>
            </marker>
          </defs>
          <path d="M 16 24 C 50 24, 50 78, 86 78"
            stroke="#10b981" strokeWidth="1.4" fill="none"
            strokeDasharray="4 4"
            markerEnd={"url(#arr-drag-"+hypId+")"}/>
        </svg>
      )}
    </div>
  );
}

function Sheet2(){
  return (
    <section className="sheet" id="sheet-2">
      <div className="sheet-head">
        <span className="sheet-num mono">SHEET 02</span>
        <h2 className="sheet-title">Port row — design hypotheses</h2>
      </div>
      <p className="sheet-desc">
        Each hypothesis is a self-consistent treatment of pillule shape, chevron form, and hit-target boundary.
        Direction is signaled by the chevron orientation, not by the side; the row works identically on any of
        the four edges. The drop-target column shows the hover-driven valid-input highlight; the debug column
        outlines the tolerance halo that catches near-misses.
      </p>

      <div className="port-sheet">
        <div className="col-h"/>
        {PORT_COLS.map(col => (
          <div key={col.tag} className="col-h">
            <span>{col.tag}</span>
            <span className="col-sub">{col.sub}</span>
          </div>
        ))}

        {PORT_HYPS.map((h, i) => (
          <React.Fragment key={h.id}>
            <div className="ph">
              <span className="ph-tag mono">{`P${i+1}`}</span>
              <span className="ph-title">{h.name}</span>
              <span className="ph-cap">{h.cap}</span>
            </div>
            {PORT_COLS.map(col => (
              <PortRowCell key={h.id + col.tag} col={col} hypId={h.id}/>
            ))}
          </React.Fragment>
        ))}
      </div>
    </section>
  );
}

/* ---------------- Sheet 3 ---------------- */

function Sheet3(){
  const cols = [
    { tag: '01', name: 'Running, not selected',
      cap: 'Status cadre alone — the canvas baseline during a Run.', status: 'running', selected: false },
    { tag: '02', name: 'Running, selected',
      cap: 'Inspector opened. Selection must read alongside the running cadre.', status: 'running', selected: true },
    { tag: '03', name: 'Failed, selected',
      cap: 'Worst-case stack: failure overlay + status cadre + selection.', status: 'failed', selected: true },
  ];
  return (
    <section className="sheet" id="sheet-3">
      <div className="sheet-head">
        <span className="sheet-num mono">SHEET 03</span>
        <h2 className="sheet-title">Selection cohabitation — focused detail</h2>
      </div>
      <p className="sheet-desc">
        Status cadre and selection signal must coexist without one swallowing the other. This sheet zooms in
        on the worst cases — running selected and failed selected — across all five status hypotheses.
      </p>

      <div className="sel-sheet">
        <div className="col-h"/>
        {cols.map(c => (
          <div key={c.tag} className="col-h">
            <span>{c.tag}</span>
            <span className="col-sub">{c.name}</span>
            <span className="col-cap">{c.cap}</span>
          </div>
        ))}

        {HYPOTHESES.map((h, i) => (
          <React.Fragment key={h.id}>
            <div className="ph">
              <span className="ph-tag mono">{`H${i+1}`}</span>
              <span className="ph-title">{h.name}</span>
              <span className="ph-cap">{h.cap}</span>
            </div>
            {cols.map(c => (
              <div key={h.id + c.tag} className={"cell " + h.id} style={{minHeight: 160}}>
                <NodeCard
                  status={c.status}
                  selected={c.selected}
                  kind="code"
                  name="rewrite_section"
                  nid="nd_4f2a"
                  ports={{ in: ['in'], out: ['out'] }}
                  hypothesisPort="p2"
                  failOverlay={h.id === 'h2' || h.id === 'h4' ? 'badge+tint' : 'badge'}
                />
              </div>
            ))}
          </React.Fragment>
        ))}
      </div>
    </section>
  );
}

/* ---------------- Document ---------------- */

function App(){
  return (
    <div className="expl-doc">
      <header className="expl-head">
        <div>
          <div className="eyebrow mono">PDO · ITERATION</div>
          <h1>Node card status outline + port row variations</h1>
        </div>
        <div className="sub">
          Components in isolation. Five status-outline hypotheses, five port-row hypotheses, and a selection-cohabitation detail.
          Pick one column from sheet 1 and one row from sheet 2; we'll integrate in the next pass.
        </div>
      </header>

      <nav className="toc">
        <a href="#sheet-1"><span className="ord">01</span> Status × treatment</a>
        <a href="#sheet-2"><span className="ord">02</span> Port row hypotheses</a>
        <a href="#sheet-3"><span className="ord">03</span> Selection cohabitation</a>
      </nav>

      <Sheet1/>
      <Sheet2/>
      <Sheet3/>

      <div className="outro">
        <div>
          <h5>Out of scope, by design</h5>
          <ul>
            <li>Full canvas chrome, panels, toolbar, run overlay.</li>
            <li>Edge / wire visual redesign.</li>
            <li>Inspector forms (output schema, port creation).</li>
            <li>Light theme, mobile.</li>
          </ul>
        </div>
        <div>
          <h5>Themable hooks</h5>
          <ul>
            <li><span className="mono">--nc-cadre-w</span> — cadre stroke width</li>
            <li><span className="mono">--nc-halo-gap</span>, <span className="mono">--nc-halo-w</span> — H4 outer ring</li>
            <li><span className="mono">--nc-glow</span> — H2 outer glow radius</li>
            <li>Pulse / march durations live in <span className="mono">@keyframes</span> nc-breath / nc-march</li>
          </ul>
        </div>
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App/>);
