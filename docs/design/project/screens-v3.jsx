// screens-v3.jsx — run lifecycle · multi-repo launch · image artifacts · diffs
// 12 new screens building on the unified-mode canvas established in screens-v2.jsx

// ── V3 icons ─────────────────────────────────────────────────────────────

function IcStop({ size, ...p }) {
  const s = size || 12;
  return <svg width={s} height={s} viewBox="0 0 12 12" fill="currentColor" {...p}><rect x="2" y="2" width="8" height="8" rx="1.5"/></svg>;
}
function IcPause(p) {
  return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" {...p}><path d="M4.5 3v8M9.5 3v8"/></svg>;
}
function IcResumePlay(p) {
  return <svg width="13" height="13" viewBox="0 0 13 13" fill="currentColor" {...p}><path d="M4 2.5l7 4-7 4z"/></svg>;
}
function IcRetryAll(p) {
  return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" {...p}><path d="M2.5 7a4.5 4.5 0 0 1 8-2.8"/><path d="M11.5 7a4.5 4.5 0 0 1-8 2.8"/><path d="M9 2.5v2.5h2.5M5 11.5V9H2.5"/></svg>;
}
function IcStaleIcon(p) {
  return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" {...p}><circle cx="7" cy="7" r="5.5"/><path d="M7 4.5v3l2 1.5"/></svg>;
}
function IcUpload(p) {
  return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" {...p}><path d="M7 9.5V2.5M4 5.5l3-3 3 3"/><path d="M2 11h10"/></svg>;
}
function IcPhoto(p) {
  return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" {...p}><rect x="1.5" y="2.5" width="11" height="9" rx="1"/><circle cx="5" cy="6" r="1.3"/><path d="M1.5 9.5l3-2.5 2.5 2L9 7l3.5 4"/></svg>;
}
function IcDiff(p) {
  return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" {...p}><path d="M3 5h8M3 9h8M9 3l2 2-2 2M5 7l-2 2 2 2"/></svg>;
}
function IcRepo(p) {
  return <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" {...p}><path d="M3 1.5h8a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1z"/><path d="M5.5 1.5v11"/><path d="M5.5 5l2-2"/><path d="M5.5 9l2-2"/></svg>;
}

// ── Data ──────────────────────────────────────────────────────────────────

const V3_LABELED_RUNS = [
  { id:'run-2026-05-14-1024-f7c', pipeline:'simple-bugfix',        status:'running', label:'Fix auth middleware bug',         when:'8 min ago',  elapsed:'08:14' },
  { id:'run-2026-05-14-0950-3ae', pipeline:'feature-pipeline',     status:'paused',  label:null,                              when:'22 min ago', elapsed:'14:02' },
  { id:'run-2026-05-14-0832-9bb', pipeline:'security-audit',       status:'stopped', label:'Audit CVE-2026-11204 patches',    when:'1 h ago',    elapsed:'21:33' },
  { id:'run-2026-05-06-0902-d11', pipeline:'doc-refresh',          status:'done',    label:'Refresh API docs + changelog',    when:'3 h ago',    elapsed:'08:22' },
  { id:'run-2026-05-05-1740-44e', pipeline:'feature-with-review',  status:'done',    label:'Add CSV export to reports',       when:'17 h ago',   elapsed:'12:47' },
];

const V3_RUN_BUGFIX = { id:'run-2026-05-14-1024-f7c', pipeline:'simple-bugfix', status:'running', label:'Fix auth middleware bug', when:'8 min ago',  elapsed:'08:14', iter:2 };
const V3_RUN_PAUSED = { id:'run-2026-05-14-0950-3ae', pipeline:'feature-with-review', status:'paused',  label:null, when:'22 min ago', elapsed:'14:02' };

// ── Canvas helper ─────────────────────────────────────────────────────────

function buildV3Nodes(overrides) {
  return (FWR_NODES || []).map(n => ({
    ...n,
    x: n.x + 200,
    ...(overrides[n.id] || {}),
  }));
}

function V3Canvas({ overrides={}, selectedId=null, run, isPaused=false, cascadeNodeId=null, cascadeText=null, children }) {
  const nodes = buildV3Nodes(overrides);
  const runningSet = new Set(nodes.filter(n => n.status==='done'||n.status==='running').map(n=>n.id));
  const sx=30, sy=215;
  const fakeStart = {id:'__start', x:sx-56, y:sy-35, status:'running'};
  const fakeEdge  = {id:'se-plan', from:'__start', to:'plan'};
  const cascNode  = cascadeNodeId ? nodes.find(n=>n.id===cascadeNodeId) : null;
  return (
    <div className="dag-canvas">
      <div className="dag-inner" style={{transform:'translate(20px,0)'}}>
        <Edges nodes={[...nodes, fakeStart]}
          edges={[...FWR_EDGES, fakeEdge]}
          runningSet={new Set([...runningSet,'__start'])}/>
        <StartNode x={sx} y={sy} when={run.when} runIdSlug={run.id.slice(-8)} downstreamRunning={runningSet.size>0}/>
        {nodes.map(n => <Node key={n.id} node={n} selected={n.id===selectedId}/>)}
        {isPaused && (
          <div className="paused-canvas-pill">
            <span className="pcp-dot"/>
            paused — running nodes will finish, no new spawns
          </div>
        )}
        {cascNode && (
          <div className="cascade-note" style={{left:cascNode.x+100, top:cascNode.y-24}}>
            <span className="cn-dot"/>
            {cascadeText||'downstream nodes reset to pending'}
          </div>
        )}
      </div>
      {children}
      <CanvasToolbar activeTool="select"/>
      <MiniMap nodes={nodes}/>
      <CanvasControls/>
    </div>
  );
}

// ── Node lifecycle controls ───────────────────────────────────────────────

function NodeLifecycleControls({ status='running' }) {
  const isRunning = status==='running';
  const isStale   = status==='stale';
  const canStop   = isRunning||isStale;
  return (
    <div className="nlc-wrap">
      <div className="nlc-eyebrow">Controls</div>
      <div className="nlc-btns">
        <button className={"btn " + (isRunning ? 'warn' : 'primary')} style={{justifyContent:'center',gap:5}}>
          {isRunning
            ? <><Ic.Refresh style={{width:11,height:11}}/> Retry</>
            : <><Ic.Play style={{width:11,height:11}}/> Play</>
          }
        </button>
        <button className={"btn " + (canStop ? 'danger' : 'ghost')} disabled={!canStop} style={{justifyContent:'center',gap:5}}>
          <IcStop size={11}/> Stop
        </button>
      </div>
      <div className="nlc-hints">
        <div className="nlc-hint">
          {isRunning ? 'kills iter · starts fresh'
            : isStale ? 'kills session · starts fresh'
            : status==='stopped' ? 'node stopped · ready to play'
            : 'start this node'}
        </div>
        <div className="nlc-hint">{canStop ? 'stops cleanly · no error' : 'not running'}</div>
      </div>
    </div>
  );
}

// ── Run overlay V3 ────────────────────────────────────────────────────────

function RunOverlayV3({ run, isPaused=false }) {
  const isActive   = run.status==='running'||run.status==='paused';
  const isTerminal = ['done','failed','archived','stopped'].includes(run.status);
  const badgeCls   = run.status==='paused' ? 'paused' : run.status==='running' ? 'running' : run.status==='failed' ? 'failed' : 'done';
  return (
    <div className="run-overlay">
      <div className="ro-head">
        <span className={"st-dot " + run.status}/>
        <span className="ro-title">{run.pipeline}</span>
        <span className={"badge " + badgeCls}>{run.status}</span>
      </div>
      {run.label && (
        <div style={{padding:'4px 0 4px',fontSize:12.5,fontWeight:600,color:'var(--fg)',letterSpacing:'-0.005em',lineHeight:1.3}}>
          {run.label}
        </div>
      )}
      <div className="ro-row"><span className="ro-label">run-id</span><span className="ro-id">{run.id.slice(-12)} <Ic.Copy/></span></div>
      <div className="ro-row"><span className="ro-label">started</span><span className="ro-value mono">{run.when}</span></div>
      <div className="ro-row"><span className="ro-label">elapsed</span>
        <span className="ro-value mono" style={run.status==='running'?{color:'var(--st-running)'}:{}}>{run.elapsed}</span>
      </div>
      {/* Lifecycle group */}
      <div className="ro-group-sep"><span className="ro-group-lbl">lifecycle</span></div>
      <div className="ro-pair">
        <button className={"btn sm" + (isPaused?' ghost':'')}
          disabled={isPaused}
          style={isPaused?{justifyContent:'center',gap:5,opacity:0.38}:{justifyContent:'center',gap:5}}>
          <IcPause style={{width:11,height:11}}/> Pause
        </button>
        <button className="btn sm"
          disabled={!isPaused}
          style={isPaused
            ? {justifyContent:'center',gap:5,background:'var(--st-paused)',color:'#03252a',border:'1px solid var(--st-paused)'}
            : {justifyContent:'center',gap:5,opacity:0.38}}>
          <IcResumePlay style={{width:10,height:10}}/> Resume
        </button>
      </div>
      <button className="btn ghost sm" style={{marginTop:4,justifyContent:'center',width:'100%',gap:6}}>
        <IcRetryAll style={{width:11,height:11}}/> Retry all
      </button>
      {/* Admin group */}
      <div className="ro-group-sep" style={{marginTop:8}}><span className="ro-group-lbl">admin</span></div>
      <button className="btn" style={{justifyContent:'center',width:'100%'}}><Ic.Manager/> Open Manager</button>
      {isActive && !isPaused && (
        <button className="btn warn" style={{marginTop:4,justifyContent:'center',width:'100%'}}><Ic.X/> Cancel</button>
      )}
      {isTerminal && <button className="btn" style={{marginTop:4,justifyContent:'center',width:'100%'}}>Cleanup</button>}
    </div>
  );
}

// ── Banners ───────────────────────────────────────────────────────────────

function StaleBanner() {
  return (
    <div className="stale-banner">
      <div className="stb-title"><IcStaleIcon style={{width:12,height:12}}/> Agent idle &mdash; &gt;2 min</div>
      Session still attached but Claude has stopped producing output. Artifacts are missing or incomplete.
      Use Stop or Retry to intervene, or attach the terminal to investigate.
    </div>
  );
}

function StoppedBanner({ downstreamCount=2 }) {
  return (
    <div className="stopped-banner">
      <div className="spb-title"><IcStop size={11}/> Stopped by user</div>
      Node was deliberately stopped. {downstreamCount} downstream node{downstreamCount!==1?'s':''} reset to
      <span className="mono" style={{color:'var(--fg-3)'}}> pending</span>.
      Use Play to restart from this node.
    </div>
  );
}

// ── Left panel with labeled run rows ─────────────────────────────────────

function LeftPanelV3({ selectedId }) {
  return (
    <>
      <PanelHead title="Runs" count={V3_LABELED_RUNS.length}
        actions={<button className="btn primary sm"><Ic.PlusSm/> New Run</button>}/>
      <div className="lp-tabs">
        <button className="lp-tab on"><Ic.Play/> Runs <span className="lp-tab-c">{V3_LABELED_RUNS.length}</span></button>
        <button className="lp-tab"><Ic.Bookmark/> Library</button>
      </div>
      <div style={{padding:'8px 12px',display:'flex',gap:6,borderBottom:'1px solid var(--line-soft)',flexWrap:'wrap'}}>
        {['All','Active','Done','Failed'].map((f,i) => (
          <button key={f} className="filter-chip" style={i===0?{color:'var(--fg)',borderColor:'var(--bg-5)',background:'var(--bg-3)'}:{}}>{f}</button>
        ))}
      </div>
      <div className="p-body">
        <div className="runs-list">
          {V3_LABELED_RUNS.map(r => (
            <div key={r.id} className={"run-row" + (r.id===selectedId?' selected':'')}>
              <span className={"st-dot " + r.status} style={{marginTop:6}}/>
              <div className="rr-main">
                <div className="rr-label">{r.label || r.id.slice(-16)}</div>
                <div className="rr-pipe">{r.pipeline}</div>
                <div className="rr-time">{r.when} · {r.elapsed}</div>
              </div>
              <div style={{display:'flex',flexDirection:'column',alignItems:'flex-end',gap:3,flexShrink:0}}>
                <button className="icon-btn" style={{width:22,height:22}}><Ic.Kebab/></button>
                <button className="rr-edit"><Ic.Pencil style={{width:10,height:10}}/></button>
              </div>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}

// ── Diff section ──────────────────────────────────────────────────────────

const DIFF_DATA = [
  { name:'src/auth/middleware.ts', add:12, del:3, hunks:[
    { hdr:'@@ -44,10 +44,18 @@ class AuthMiddleware {', lines:[
      {t:'ctx',  l:44, r:44, s:' ', txt:"  async validateToken(token: string) {"},
      {t:'del',  l:45, r:null, s:'-', txt:'    throw new Error("auth failed")'},
      {t:'del',  l:46, r:null, s:'-', txt:'    return null'},
      {t:'add',  l:null, r:45, s:'+', txt:'    const result = await this.verifier.verify(token)'},
      {t:'add',  l:null, r:46, s:'+', txt:'    if (!result.valid) {'},
      {t:'add',  l:null, r:47, s:'+', txt:'      throw new AuthError(result.reason)'},
      {t:'add',  l:null, r:48, s:'+', txt:'    }'},
      {t:'add',  l:null, r:49, s:'+', txt:'    return result.claims'},
      {t:'ctx',  l:47, r:50, s:' ', txt:'  }'},
    ]}
  ]},
  { name:'src/auth/errors.ts', add:8, del:0, hunks:[
    { hdr:'@@ -0,0 +1,8 @@', lines:[
      {t:'add', l:null, r:1, s:'+', txt:'export class AuthError extends Error {'},
      {t:'add', l:null, r:2, s:'+', txt:'  constructor(public readonly reason: string) {'},
      {t:'add', l:null, r:3, s:'+', txt:'    super(`Auth failed: ${reason}`)'},
      {t:'add', l:null, r:4, s:'+', txt:'  }'},
      {t:'add', l:null, r:5, s:'+', txt:'}'},
    ]}
  ]},
  { name:'test/auth.test.ts', add:6, del:2, hunks:[
    { hdr:"@@ -12,4 +12,9 @@ describe('AuthMiddleware')", lines:[
      {t:'ctx', l:12, r:12, s:' ', txt:"  it('rejects bad tokens', async () => {"},
      {t:'del', l:13, r:null, s:'-', txt:"    expect(() => m.validateToken('bad')).toThrow()"},
      {t:'add', l:null, r:13, s:'+', txt:"    await expect(m.validateToken('bad')).rejects.toBeInstanceOf(AuthError)"},
      {t:'add', l:null, r:14, s:'+', txt:"    await expect(m.validateToken('exp')).rejects.toMatchObject({"},
      {t:'add', l:null, r:15, s:'+', txt:"      reason: 'TOKEN_EXPIRED'"},
      {t:'add', l:null, r:16, s:'+', txt:"    })"},
      {t:'ctx', l:14, r:17, s:' ', txt:'  })'},
    ]}
  ]},
];

function DiffSection({ expanded=true }) {
  const totalAdd = DIFF_DATA.reduce((s,f)=>s+f.add,0);
  const totalDel = DIFF_DATA.reduce((s,f)=>s+f.del,0);
  return (
    <div className="diff-sect">
      <div className={"diff-head" + (expanded?'':' closed')}>
        <IcDiff style={{color:'var(--fg-4)',flexShrink:0,width:13,height:13}}/>
        <span className="dh-lbl">Diff</span>
        <span className="dh-add">+{totalAdd}</span>
        <span className="dh-del">-{totalDel}</span>
        <span style={{marginLeft:'auto',color:'var(--fg-4)',fontSize:10}}>3 files changed</span>
        <Ic.Chevron style={{color:'var(--fg-4)',transform:expanded?'':'rotate(-90deg)'}}/>
      </div>
      {expanded && (
        <>
          <div className="diff-node-bar">
            <span className="dnb-lbl">per-node:</span>
            <select defaultValue="all">
              <option value="all">aggregate (all nodes)</option>
              <option value="impl">Implementer · impl</option>
              <option value="review">Reviewer · review</option>
            </select>
          </div>
          <div className="diff-body">
            {DIFF_DATA.map((f,fi) => (
              <div key={fi} className="diff-file">
                <div className="diff-fhead">
                  <span>{f.name}</span>
                  <span className="dfh-stat"><span className="dfa">+{f.add}</span> <span className="dfd">-{f.del}</span></span>
                </div>
                {f.hunks.map((h,hi) => (
                  <div key={hi}>
                    <div className="diff-hunk">{h.hdr}</div>
                    {h.lines.map((l,li) => (
                      <div key={li} className={"diff-line " + l.t}>
                        <span className="dl-lno">{l.l||''}</span>
                        <span className="dl-lno">{l.r||''}</span>
                        <span className="dl-sign">{l.s}</span>
                        <span className="dl-txt">{l.txt}</span>
                      </div>
                    ))}
                  </div>
                ))}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

// ── Image gallery ─────────────────────────────────────────────────────────

const IMG_ITEMS = [
  {name:'login-flow-before.png',  desc:'Login page — before patch'},
  {name:'auth-error-modal.png',   desc:'Auth error modal — state capture'},
  {name:'login-flow-after.png',   desc:'Login page — after patch'},
];

function ImageGallery({ lightboxIdx=null }) {
  const hasLightbox = lightboxIdx !== null;
  return (
    <div style={{position:'relative'}}>
      <div className="img-gallery">
        {IMG_ITEMS.map((img,i) => (
          <div key={i} className="img-gitem" style={i===lightboxIdx?{borderColor:'var(--acc)'}:{}}>
            <div className="igi-stripe"/>
            <div className="igi-label">{img.name}</div>
            <div className="igi-zoom"><Ic.Maximize style={{width:10,height:10}}/></div>
          </div>
        ))}
      </div>
      {hasLightbox && (
        <div className="img-lightbox">
          <div className="img-lb-head">
            <IcPhoto style={{color:'var(--fg-4)',width:13,height:13,flexShrink:0}}/>
            <span className="ilbh-name">{IMG_ITEMS[lightboxIdx].name}</span>
            <span style={{color:'var(--fg-4)',fontSize:10.5,flexShrink:0}}>{lightboxIdx+1} / {IMG_ITEMS.length}</span>
            <button className="icon-btn" style={{width:22,height:22,marginLeft:4}}><Ic.X/></button>
          </div>
          <div className="img-lb-body">
            <div className="img-lb-img">
              <div className="ilbi-stripe"/>
              <div className="ilbi-label">{IMG_ITEMS[lightboxIdx].desc}</div>
            </div>
          </div>
          <div style={{padding:'8px 12px',borderTop:'1px solid var(--line)',display:'flex',gap:6}}>
            <button className="btn ghost sm" style={{gap:5}}>
              <Ic.ChevronR style={{transform:'rotate(180deg)',width:11,height:11}}/> Prev
            </button>
            <button className="btn ghost sm" style={{gap:5}}>
              Next <Ic.ChevronR style={{width:11,height:11}}/>
            </button>
            <button className="btn ghost sm" style={{marginLeft:'auto',gap:5}}>
              <Ic.External style={{width:11,height:11}}/> Full size
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Markdown viewer with inline images ────────────────────────────────────

function MdViewerImages() {
  return (
    <div className="md-bg">
      <div className="md-modal" style={{width:660}}>
        <div className="md-head">
          <div className="md-title">
            <div className="md-port">diff</div>
            <div className="md-path">artifacts/impl/iter-2/diff.md</div>
          </div>
          <div/>
          <div className="md-close-wrap">
            <button className="icon-btn"><Ic.X/></button>
          </div>
        </div>
        <div className="md-body">
          <div className="frontmatter">
            <span className="k">summary</span><span className="v">"fix auth middleware — JWT verify path"</span>
            <span className="k">files_changed</span><span className="v enum-pass">7</span>
            <span className="k">tests_added</span><span className="v enum-pass">4</span>
            <span className="k">verdict</span><span className="v enum-pass">PASS</span>
          </div>
          <hr className="md-divider"/>
          <div className="md-render">
            <h1>Implementation Summary</h1>
            <p>This iteration resolves the broken JWT validation path in the auth middleware. The previous implementation threw synchronously before the verification logic could run.</p>
            <h2>Changes Made</h2>
            <p>The core fix replaces the <code>throw new Error</code> stub with a real call to <code>this.verifier.verify(token)</code>, propagating the reason from the verifier into the new <code>AuthError</code> class.</p>
            <div className="md-inline-img">
              <div className="mdi-stripe"/>
              <div className="mdi-label">screenshot · login flow before patch</div>
              <div className="mdi-caption">login-flow-before.png</div>
            </div>
            <p>After applying the patch the login flow completes cleanly. The error modal now surfaces a user-readable reason string instead of the generic <code>auth failed</code> message.</p>
            <div className="md-inline-img">
              <div className="mdi-stripe"/>
              <div className="mdi-label">screenshot · login flow after patch</div>
              <div className="mdi-caption">login-flow-after.png</div>
            </div>
            <h2>Tests</h2>
            <ul>
              <li>4 new cases covering the error propagation path</li>
              <li>All existing tests continue to pass</li>
              <li>Type-check clean on <code>strict</code> and <code>noUncheckedIndexedAccess</code></li>
            </ul>
          </div>
        </div>
        <div className="md-foot">
          <span>iter 2 of 5 · 3.4 KB</span>
          <span>2 inline images · <span className="mono">image_list</span> not present</span>
        </div>
      </div>
    </div>
  );
}

// ── Output type selector port card ────────────────────────────────────────

function OutputPortV3({ portName, portType='markdown' }) {
  const types = ['markdown','image','image_list'];
  const hasSchema = portType==='markdown';
  const isImageList = portType==='image_list';
  return (
    <div className="sp3">
      <div className="sp3-head">
        <span className="pdot" style={{width:8,height:8,borderRadius:4,background:isImageList?'var(--acc)':'var(--fg-5)',flexShrink:0,marginTop:3}}/>
        <div style={{flex:1,minWidth:0}}>
          <div className="pname">{portName}</div>
          <div className="pmono">
            {portType==='image_list' ? 'image gallery · alphabetical'
              : portType==='image' ? 'single image'
              : 'markdown + frontmatter schema'}
          </div>
        </div>
        <div style={{display:'flex',alignItems:'center',gap:6,flexShrink:0}}>
          <div className="port-type-seg">
            {types.map(t => (
              <button key={t} className={"port-type-btn" + (t===portType?' on':'')}>{t}</button>
            ))}
          </div>
          <button className="icon-btn" style={{width:22,height:22}}><Ic.Kebab/></button>
        </div>
      </div>
      {hasSchema && (
        <div className="schema-rows">
          <div className="schema-row">
            <input className="input mono sm" defaultValue="summary"/>
            <select className="select mono sm" defaultValue="string"><option>string</option><option>int</option><option>bool</option><option>enum</option></select>
            <button className="icon-btn" style={{width:22,height:22}}><Ic.X/></button>
          </div>
          <div className="schema-row">
            <input className="input mono sm" defaultValue="files_changed"/>
            <select className="select mono sm" defaultValue="int"><option>int</option><option>string</option><option>enum</option></select>
            <button className="icon-btn" style={{width:22,height:22}}><Ic.X/></button>
          </div>
          <div className="schema-row enum-row focused">
            <input className="input mono sm" defaultValue="verdict"/>
            <select className="select mono sm" defaultValue="enum"><option>enum</option><option>string</option><option>int</option></select>
            <button className="icon-btn" style={{width:22,height:22}}><Ic.X/></button>
            <div className="enum-allowed">
              <span className="ea-label mono">allowed:</span>
              <span className="ea-chip">PASS <button>×</button></span>
              <span className="ea-chip">FAIL <button>×</button></span>
              <span className="ea-chip">NEEDS_WORK <button>×</button></span>
              <button className="ea-add">+ value</button>
            </div>
          </div>
          <button className="btn ghost sm" style={{marginTop:5}}><Ic.PlusSm/> Add field</button>
        </div>
      )}
      {isImageList && (
        <div style={{fontSize:11.5,color:'var(--fg-4)',lineHeight:1.5,padding:'2px 0'}}>
          Directory listing drives gallery order.
          No frontmatter schema — images are read directly from
          <span className="mono" style={{color:'var(--fg-3)'}}> artifacts/{portName}/iter-*/</span>.
        </div>
      )}
    </div>
  );
}

// ── New Run modal V3 ──────────────────────────────────────────────────────

const REPO_PIPELINES_V3 = [
  {id:'simple-bugfix',   nodes:4, mod:'1 d ago', sel:true},
  {id:'feature-flow',    nodes:6, mod:'3 d ago'},
];
const LIB_PIPELINES_V3 = [
  {id:'feature-with-review', nodes:5, mod:'2 d ago', star:true},
  {id:'bug-triage',          nodes:4, mod:'5 d ago', star:true},
  {id:'doc-refresh',         nodes:3, mod:'1 wk ago', star:false},
];

function NewRunModalV3({ showImages=false }) {
  return (
    <div className="modal-bg">
      <div className="modal modal-v3">
        <div className="modal-head">
          <h2>New Run</h2>
          <button className="icon-btn"><Ic.X/></button>
        </div>
        <div className="modal-body">
          {/* ─ WHERE ─ */}
          <div className="msec">
            <div className="msec-lbl">Where</div>
            <div className="field">
              <label>Target repository</label>
              <button className="repo-btn">
                <span className="rb-icon"><IcRepo style={{width:12,height:12}}/></span>
                <span className="rb-val">/Users/alex/code/myapp</span>
                <span className="mono" style={{fontSize:10,color:'var(--fg-4)'}}>· git · main</span>
                <Ic.Chevron className="rb-caret" style={{transform:'rotate(-90deg)'}}/>
              </button>
            </div>
            <div className="field" style={{marginBottom:0}}>
              <label>Source branch</label>
              <div className="branch-row">
                <select defaultValue="feature/fix-auth-middleware">
                  <option>main</option>
                  <option value="feature/fix-auth-middleware">feature/fix-auth-middleware</option>
                  <option>feat/new-dashboard</option>
                  <option>bugfix/csv-export</option>
                </select>
                <span className="br-meta">HEAD · 3 commits ahead of main</span>
              </div>
            </div>
          </div>
          {/* ─ HOW ─ */}
          <div className="msec">
            <div className="msec-lbl">How</div>
            <div className="field" style={{marginBottom:0}}>
              <label>Pipeline</label>
              <div className="picker open">
                <button className="picker-btn">
                  <span className="badge repo" style={{marginRight:2}}>repo</span>
                  <span className="mono" style={{color:'var(--fg)'}}>simple-bugfix</span>
                  <span className="mono" style={{color:'var(--fg-4)',fontSize:10.5}}>· 4 nodes · 1 d ago</span>
                  <span className="spacer"/>
                  <Ic.Chevron style={{transform:'rotate(-90deg)'}}/>
                </button>
                <div className="picker-list">
                  <div className="pkv3-grp-head">
                    <IcRepo style={{color:'var(--fg-4)',width:11,height:11}}/> from this repo
                  </div>
                  {REPO_PIPELINES_V3.map(p => (
                    <div key={p.id} className={"picker-row" + (p.sel?' on':'')}>
                      <span className="badge repo" style={{minWidth:36,justifyContent:'center'}}>repo</span>
                      <span className="mono" style={{color:'var(--fg)',flex:1}}>{p.id}</span>
                      <span className="mono" style={{color:'var(--fg-4)',fontSize:10.5}}>{p.nodes} nodes · {p.mod}</span>
                    </div>
                  ))}
                  <div className="pkv3-grp-head" style={{marginTop:4}}>
                    <Ic.Bookmark style={{color:'var(--fg-4)',width:11,height:11}}/> library templates
                  </div>
                  {LIB_PIPELINES_V3.map(p => (
                    <div key={p.id} className="picker-row">
                      {p.star
                        ? <Ic.StarFill style={{color:'var(--st-await)',flexShrink:0}}/>
                        : <Ic.Star style={{color:'var(--fg-4)',flexShrink:0}}/>
                      }
                      <span className="mono" style={{color:'var(--fg)',flex:1}}>{p.id}</span>
                      <span className="mono" style={{color:'var(--fg-4)',fontSize:10.5}}>{p.nodes} nodes · {p.mod}</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
          {/* ─ WHAT ─ */}
          <div className="msec">
            <div className="msec-lbl">What</div>
            <div className="field">
              <label>Name</label>
              <div className="autocheck">
                <input type="checkbox" defaultChecked id="v3-autogen"/>
                <label htmlFor="v3-autogen" style={{cursor:'pointer'}}>Auto-generated by manager</label>
              </div>
            </div>
            <div className="field">
              <label>Prompt</label>
              <textarea className="textarea mono" rows={4}
                defaultValue="The auth middleware is broken. JWT tokens from the new provider are being rejected with a generic error. Trace the issue, fix it, and add tests covering the new error path."/>
            </div>
            <div className="field" style={{marginBottom:0}}>
              <label>Images <span style={{color:'var(--fg-4)',fontWeight:400}}>(optional)</span></label>
              {showImages ? (
                <>
                  <div className="img-previews">
                    <div className="img-prev-card">
                      <div className="ipc-stripe"/>
                      <div className="ipc-name">auth-error-modal.png</div>
                      <button className="ipc-del">×</button>
                    </div>
                    <div className="img-prev-card">
                      <div className="ipc-stripe"/>
                      <div className="ipc-name">login-debug-screenshot.png</div>
                      <button className="ipc-del">×</button>
                    </div>
                  </div>
                  <button className="btn ghost sm" style={{marginTop:6,gap:5}}><IcUpload style={{width:11,height:11}}/> Add more</button>
                </>
              ) : (
                <div className="img-drop">
                  <div className="id-icon"><IcUpload/></div>
                  <div className="id-lbl">Drag and drop, paste from clipboard, or click to upload</div>
                  <div className="id-sub">PNG · JPG · GIF · WebP supported</div>
                </div>
              )}
            </div>
          </div>
          {/* ─ CONFIG ─ */}
          <div className="msec">
            <div className="accord">
              <div className="accord-head">
                <Ic.Chevron style={{transform:'rotate(-90deg)'}}/>
                <span style={{fontSize:12,fontWeight:500}}>Variables</span>
                <span className="acc-count">3</span>
              </div>
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

// ─────────────────────────────────────────────────────────────────────────
// SCREENS
// ─────────────────────────────────────────────────────────────────────────

// A1 — Running node: lifecycle controls at top of inspector
function ScreenV3_A1() {
  const run = V3_RUN_BUGFIX;
  return (
    <div className="artboard-host" data-screen-label="01 Running node + lifecycle controls">
      <Frame mode="run" breadcrumb="simple-bugfix" runId={run.id.slice(-8)} activeRuns={2} awaiting={0}>
        <div className="panel panel-l"><LeftPanelV3 selectedId={run.id}/></div>
        <div className="panel panel-c">
          <V3Canvas run={run}
            overrides={{plan:{status:'done'},impl:{status:'running',iter:'2/5'},review:{status:'running',iter:'2/5'},tests:{status:'pending'},merge:{status:'pending'}}}
            selectedId="impl">
            <RunOverlayV3 run={run}/>
          </V3Canvas>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <div className="p-body">
            <div className="p-sect" style={{paddingBottom:10}}>
              <div style={{display:'flex',alignItems:'center',gap:8,marginBottom:6}}>
                <span className="st-dot running"/>
                <div style={{flex:1,minWidth:0}}>
                  <div style={{fontSize:13,fontWeight:600}}>Implementer</div>
                  <div className="mono" style={{fontSize:11,color:'var(--fg-3)',marginTop:2}}>9k2x7m</div>
                </div>
                <span className="badge code">code</span>
                <span className="badge">iter 2/5</span>
                <button className="ih-star outline"><Ic.Star/></button>
              </div>
            </div>
            <NodeLifecycleControls status="running"/>
            <div className="p-sect">
              <SectionHead title="Terminal"/>
              <XTerm session="tmux: pdo/run-f7c/impl · 80×24" height={190}
                lines={[
                  {p:'claude › ', t:'reading src/auth/middleware.ts'},
                  {d:'  ↳ 203 lines'},
                  {tool:'read_file', arg:'src/auth/middleware.ts'},
                  {d:'  ↳ found validateToken stub at line 45'},
                  {p:'claude › ', t:'scanning for related files'},
                  {d:'  ↳ 3 files matched'},
                  {tool:'edit_file', arg:'src/auth/middleware.ts'},
                  {ok:'  ✓ patch applied (+14, -2)'},
                  {p:'claude › ', t:'writing diff.md', cursor:true},
                ]}/>
            </div>
            <div className="p-sect"><SectionHead title="Inputs" count={2} collapsed/></div>
            <div className="p-sect"><SectionHead title="Outputs" count={1} collapsed/></div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// A2 — Paused run: Resume highlighted, Pause greyed
function ScreenV3_A2() {
  const run = V3_RUN_PAUSED;
  return (
    <div className="artboard-host" data-screen-label="02 Paused run — Resume">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)} activeRuns={1} awaiting={0}>
        <div className="panel panel-l"><LeftPanelV3 selectedId={run.id}/></div>
        <div className="panel panel-c">
          <V3Canvas run={run} isPaused
            overrides={{plan:{status:'done'},impl:{status:'running',iter:'2/5'},review:{status:'pending'},tests:{status:'pending'},merge:{status:'pending'}}}
            selectedId={null}>
            <RunOverlayV3 run={run} isPaused/>
          </V3Canvas>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Pipeline info" actions={<button className="icon-btn"><Ic.X/></button>}/>
          <div className="p-body">
            <div className="p-sect pip-meta">
              <div className="pip-head">
                <span className="st-dot paused"/>
                <div style={{flex:1,minWidth:0}}>
                  <div className="pip-name">feature-with-review</div>
                  <div className="pip-sub mono">{run.id.slice(-8)} · v3 · <span style={{color:'var(--st-paused)'}}>paused</span></div>
                </div>
                <button className="ih-star synced"><Ic.StarFill/></button>
              </div>
              <div style={{padding:'10px 12px',background:'var(--st-paused-bg)',border:'1px solid rgba(34,211,238,0.25)',borderRadius:6,fontSize:11.5,color:'#a5f3fc',lineHeight:1.5}}>
                <div style={{display:'flex',alignItems:'center',gap:6,fontWeight:600,color:'var(--st-paused)',fontSize:11,textTransform:'uppercase',letterSpacing:'0.06em',marginBottom:4}}>
                  <IcPause style={{width:12,height:12}}/> Paused
                </div>
                Running nodes (1) will finish. No new nodes will spawn until resumed.
              </div>
            </div>
            <div className="p-sect" style={{borderBottom:'none',flex:1,display:'flex',flexDirection:'column'}}>
              <div className="pip-mgr-head"><Ic.Manager/><span>Pipeline Manager</span></div>
              <XTerm height={260}
                lines={[
                  {d:'[14:42:08] manager session attached'},
                  {p:'mgr › ', t:'scheduler status'},
                  {d:'  ↳ status: paused'},
                  {d:'  ↳ impl: running · iter 2/5 (finishing)'},
                  {d:'  ↳ review: pending · held'},
                  {d:'  ↳ tests: pending · held'},
                  {d:'  ↳ merge: pending · held'},
                  {warn:'  paused at 14:42:06 · use resume to continue'},
                  {p:'mgr › ', t:'', cursor:true},
                ]}/>
            </div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// A3 — Stopped node + downstream cascade
function ScreenV3_A3() {
  const run = {...V3_RUN_BUGFIX, status:'running'};
  return (
    <div className="artboard-host" data-screen-label="03 Stopped node — cascade">
      <Frame mode="run" breadcrumb="simple-bugfix" runId={run.id.slice(-8)} activeRuns={1} awaiting={0}>
        <div className="panel panel-l"><LeftPanelV3 selectedId={run.id}/></div>
        <div className="panel panel-c">
          <V3Canvas run={run}
            overrides={{plan:{status:'done'},impl:{status:'stopped',iter:'2/5'},review:{status:'pending'},tests:{status:'pending'},merge:{status:'pending'}}}
            selectedId="impl" cascadeNodeId="impl" cascadeText="2 downstream nodes reset to pending">
            <RunOverlayV3 run={run}/>
          </V3Canvas>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <div className="p-body">
            <div className="p-sect" style={{paddingBottom:10}}>
              <div style={{display:'flex',alignItems:'center',gap:8,marginBottom:6}}>
                <span className="st-dot stopped"/>
                <div style={{flex:1,minWidth:0}}>
                  <div style={{fontSize:13,fontWeight:600}}>Implementer</div>
                  <div className="mono" style={{fontSize:11,color:'var(--fg-3)',marginTop:2}}>9k2x7m</div>
                </div>
                <span className="badge code">code</span>
                <span className="badge stopped">stopped</span>
                <button className="ih-star outline"><Ic.Star/></button>
              </div>
            </div>
            <div className="p-sect" style={{paddingTop:8,paddingBottom:8}}>
              <StoppedBanner downstreamCount={2}/>
            </div>
            <NodeLifecycleControls status="stopped"/>
            <div className="p-sect">
              <SectionHead title="Terminal"/>
              <XTerm session="tmux: pdo/run-f7c/impl · 80×24" height={170}
                lines={[
                  {p:'claude › ', t:'reading src/auth/middleware.ts'},
                  {d:'  ↳ 203 lines'},
                  {tool:'read_file', arg:'src/auth/middleware.ts'},
                  {d:'  ↳ found validateToken stub at line 45'},
                  {p:'claude › ', t:'analysing error path'},
                  {d:'[14:41:22] session terminated by user'},
                ]} focused={false}/>
            </div>
            <div className="p-sect"><SectionHead title="Inputs" count={2} collapsed/></div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// A4 — Stale node detected
function ScreenV3_A4() {
  const run = {...V3_RUN_BUGFIX, when:'12 min ago', elapsed:'12:41'};
  return (
    <div className="artboard-host" data-screen-label="04 Stale node — agent idle">
      <Frame mode="run" breadcrumb="simple-bugfix" runId={run.id.slice(-8)} activeRuns={1} awaiting={0}>
        <div className="panel panel-l"><LeftPanelV3 selectedId={run.id}/></div>
        <div className="panel panel-c">
          <V3Canvas run={run}
            overrides={{plan:{status:'done'},impl:{status:'stale',iter:'2/5'},review:{status:'pending'},tests:{status:'pending'},merge:{status:'pending'}}}
            selectedId="impl">
            <RunOverlayV3 run={run}/>
          </V3Canvas>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <div className="p-body">
            <div className="p-sect" style={{paddingBottom:10}}>
              <div style={{display:'flex',alignItems:'center',gap:8,marginBottom:6}}>
                <span className="st-dot stale"/>
                <div style={{flex:1,minWidth:0}}>
                  <div style={{fontSize:13,fontWeight:600}}>Implementer</div>
                  <div className="mono" style={{fontSize:11,color:'var(--fg-3)',marginTop:2}}>9k2x7m</div>
                </div>
                <span className="badge code">code</span>
                <span className="badge stale">stale</span>
                <button className="ih-star outline"><Ic.Star/></button>
              </div>
            </div>
            <div className="p-sect" style={{paddingTop:8,paddingBottom:8}}>
              <StaleBanner/>
            </div>
            <NodeLifecycleControls status="stale"/>
            <div className="p-sect">
              <SectionHead title="Terminal"/>
              <XTerm session="tmux: pdo/run-f7c/impl · 80×24" height={200}
                lines={[
                  {p:'claude › ', t:'reading src/auth/'},
                  {d:'  ↳ 8 files · 1.4 KB total'},
                  {tool:'read_file', arg:'src/auth/middleware.ts'},
                  {d:'  ↳ 203 lines loaded'},
                  {p:'claude › ', t:''},
                  {d:'[2 min 14 s elapsed — no output]'},
                  {p:'claude › ', t:'', cursor:true},
                ]} focused={false}/>
            </div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// B5 — New Run modal: repo + branch + two-group pipeline picker
function ScreenV3_B5() {
  return (
    <div className="artboard-host" data-screen-label="05 New Run — repo + branch + pipeline groups">
      <Frame mode="run" breadcrumb="No run selected" awaiting={0} activeRuns={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="empty">
              <div className="emp-art"><Ic.Grid/></div>
              <div className="emp-title">No run selected</div>
              <div className="emp-sub">Launch a new run or select one from the left panel.</div>
            </div>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Detail"/>
          <div className="p-body"><div className="empty" style={{padding:'40px 24px'}}><div className="emp-sub" style={{maxWidth:240}}>Select a node to inspect.</div></div></div>
        </div>
      </Frame>
      <NewRunModalV3 showImages={false}/>
    </div>
  );
}

// B6 — New Run modal: image upload with thumbnails
function ScreenV3_B6() {
  return (
    <div className="artboard-host" data-screen-label="06 New Run — image upload area">
      <Frame mode="run" breadcrumb="No run selected" awaiting={0} activeRuns={0}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs"/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="empty">
              <div className="emp-art"><Ic.Grid/></div>
              <div className="emp-sub">Launch a new run.</div>
            </div>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Detail"/>
          <div className="p-body"><div className="empty" style={{padding:'40px 24px'}}><div className="emp-sub" style={{maxWidth:240}}>Select a node to inspect.</div></div></div>
        </div>
      </Frame>
      <NewRunModalV3 showImages={true}/>
    </div>
  );
}

// B7 — Left panel: two-line run rows with display labels
function ScreenV3_B7() {
  return (
    <div className="artboard-host" data-screen-label="07 Run rows — display label + pipeline name">
      <Frame mode="run" breadcrumb="No run selected" awaiting={0} activeRuns={2}>
        <div className="panel panel-l">
          <LeftPanelV3 selectedId={V3_LABELED_RUNS[0].id}/>
        </div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="empty">
              <div className="emp-art"><Ic.Grid/></div>
              <div className="emp-title">Select a run</div>
              <div className="emp-sub">Select a run from the panel to view its pipeline canvas. The edit icon on hover renames the run label.</div>
            </div>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Detail"/>
          <div className="p-body"><div className="empty" style={{padding:'40px 24px'}}><div className="emp-sub" style={{maxWidth:240}}>Select a node to inspect.</div></div></div>
        </div>
      </Frame>
    </div>
  );
}

// C8 — Pipeline info panel: aggregate run diff
function ScreenV3_C8() {
  const run = RUNS[0];
  return (
    <div className="artboard-host" data-screen-label="08 Pipeline info — aggregate diff">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)} activeRuns={2}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={run.id}/></div>
        <div className="panel panel-c">
          <V3Canvas run={run}
            overrides={{plan:{status:'done'},impl:{status:'running',iter:'2/5'},review:{status:'running',iter:'2/5'},tests:{status:'pending'},merge:{status:'pending'}}}
            selectedId={null}>
            <RunOverlayV3 run={{...run,status:'running'}}/>
          </V3Canvas>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Pipeline info" actions={<button className="icon-btn"><Ic.X/></button>}/>
          <div className="p-body">
            <div className="p-sect pip-meta" style={{paddingBottom:10}}>
              <div className="pip-head">
                <span className="st-dot running"/>
                <div style={{flex:1,minWidth:0}}>
                  <div className="pip-name">feature-with-review</div>
                  <div className="pip-sub mono">{run.id.slice(-8)} · v3</div>
                </div>
                <button className="ih-star synced"><Ic.StarFill/></button>
              </div>
              <div className="pip-vars">
                <div className="pip-var"><span className="k mono">max_iter</span><span className="v mono">5</span></div>
                <div className="pip-var"><span className="k mono">branch_prefix</span><span className="v mono">"feat/"</span></div>
              </div>
            </div>
            <div className="p-sect">
              <SectionHead title="Diff"/>
              <DiffSection expanded/>
            </div>
            <div className="p-sect" style={{borderBottom:'none',flex:1,display:'flex',flexDirection:'column'}}>
              <div className="pip-mgr-head"><Ic.Manager/><span>Pipeline Manager</span></div>
              <XTerm height={160}
                lines={[
                  {d:'[14:32:01] manager session attached'},
                  {p:'mgr › ', t:'scheduler tick'},
                  {d:'  ↳ plan: done · impl: running · review: running'},
                  {p:'mgr › ', t:'', cursor:true},
                ]}/>
            </div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// C9 — Node detail: image_list output port with gallery
function ScreenV3_C9() {
  const run = {...RUNS[0], pipeline:'visual-diff'};
  const screenshotNode = {
    id:'screenshot', nid:'ss4k8p', name:'Screenshot Agent',
    type:'code', status:'done', x:800, y:200,
    ports:{in:['diff'], out:['screenshots']},
  };
  return (
    <div className="artboard-host" data-screen-label="09 Image gallery — node outputs">
      <Frame mode="run" breadcrumb="visual-diff" runId={run.id.slice(-8)} activeRuns={1}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={run.id}/></div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="dag-inner" style={{transform:'translate(20px,0)'}}>
              <StartNode x={30} y={215} when="4 min ago" runIdSlug={run.id.slice(-8)}/>
              <Node node={{id:'plan',nid:'k7m2x9',name:'Planner',type:'doc',status:'done',x:260,y:200,ports:{in:['issue'],out:['plan']}}}/>
              <Node node={{id:'impl',nid:'9k2x7m',name:'Implementer',type:'code',status:'done',x:520,y:100,ports:{in:['plan'],out:['diff']},iter:'2/5'}}/>
              <Node node={{...screenshotNode}} selected/>
              <svg style={{position:'absolute',inset:0,width:'100%',height:'100%',pointerEvents:'none'}}>
                <defs><marker id="c9a" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="5" markerHeight="5" orient="auto"><path d="M0 0 L8 4 L0 8 z" fill="#525a68"/></marker></defs>
                <path d="M 186 243 L 260 235" stroke="#525a68" strokeWidth="1.5" fill="none" markerEnd="url(#c9a)"/>
                <path d="M 460 235 C 490 235, 490 135, 520 135" stroke="#10b981" strokeWidth="1.5" fill="none" markerEnd="url(#c9a)"/>
                <path d="M 720 135 C 760 135, 760 235, 800 235" stroke="#10b981" strokeWidth="1.5" fill="none" markerEnd="url(#c9a)"/>
              </svg>
            </div>
            <CanvasToolbar activeTool="select"/>
            <MiniMap nodes={[{id:'plan',x:260,y:200,status:'done'},{id:'impl',x:520,y:100,status:'done'},{id:'ss',x:800,y:200,status:'done'}]}/>
            <CanvasControls/>
          </div>
        </div>
        <div className="panel panel-r" style={{position:'relative',overflow:'hidden'}}>
          <PanelHead title="Screenshot Agent"/>
          <div className="p-body">
            <div className="p-sect" style={{paddingBottom:10}}>
              <div style={{display:'flex',alignItems:'center',gap:8,marginBottom:6}}>
                <span className="st-dot done"/>
                <div style={{flex:1,minWidth:0}}>
                  <div style={{fontSize:13,fontWeight:600}}>Screenshot Agent</div>
                  <div className="mono" style={{fontSize:11,color:'var(--fg-3)',marginTop:2}}>ss4k8p</div>
                </div>
                <span className="badge code">code</span>
                <span className="badge done">done</span>
              </div>
            </div>
            <div className="p-sect">
              <SectionHead title="Outputs" count={1}/>
              <div className="port-row">
                <span className="pdot ok"/>
                <div style={{flex:1,minWidth:0}}>
                  <div className="pname" style={{display:'flex',alignItems:'center',gap:6}}>
                    screenshots
                    <span className="badge" style={{height:15,padding:'0 5px',fontSize:9,background:'var(--st-paused-bg)',color:'var(--st-paused)',border:'1px solid rgba(34,211,238,0.28)'}}>image_list</span>
                  </div>
                  <div className="ppath">artifacts/screenshot/iter-1/ · 3 images</div>
                </div>
                <span className="open-link">view ↗</span>
              </div>
              <ImageGallery lightboxIdx={1}/>
            </div>
            <div className="p-sect"><SectionHead title="Inputs" count={1} collapsed/></div>
          </div>
        </div>
      </Frame>
    </div>
  );
}

// C10 — Markdown viewer with inline images
function ScreenV3_C10() {
  const run = RUNS[0];
  return (
    <div className="artboard-host" data-screen-label="10 Markdown viewer — inline images">
      <Frame mode="run" breadcrumb="feature-with-review" runId={run.id.slice(-8)}>
        <div className="panel panel-l"><UnifiedLeftPanel tab="runs" selectedRunId={run.id}/></div>
        <div className="panel panel-c">
          <V3Canvas run={run}
            overrides={{plan:{status:'done'},impl:{status:'done',iter:'2/5'},review:{status:'running',iter:'2/5'},tests:{status:'pending'},merge:{status:'pending'}}}
            selectedId="impl">
            <RunOverlayV3 run={{...run,status:'running'}}/>
          </V3Canvas>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Implementer"/>
          <NodeDetailV2 node={{...FWR_NODES[1], runSlug:'run-a3f'}}/>
        </div>
      </Frame>
      <MdViewerImages/>
    </div>
  );
}

// S11 — Output port type selector (edit inspector)
function ScreenV3_S11() {
  return (
    <div className="artboard-host" data-screen-label="11 Output port type selector">
      <div style={{width:'100%',height:'100%',background:'var(--bg-1)',display:'flex',alignItems:'flex-start',justifyContent:'center',padding:'48px 80px',overflow:'auto'}}>
        <div style={{width:490}}>
          <div style={{textAlign:'center',marginBottom:28}}>
            <div style={{fontSize:14,fontWeight:600,color:'var(--fg)'}}>Output port type selector</div>
            <div style={{fontSize:11.5,color:'var(--fg-4)',marginTop:4,lineHeight:1.55}}>
              Per-port type in the node inspector (edit context).
              <span className="mono" style={{color:'var(--fg-3)'}}> markdown</span> ports keep their frontmatter schema.
              <span className="mono" style={{color:'var(--fg-3)'}}> image_list</span> ports show a directory listing instead.
            </div>
          </div>
          <div className="pdo" style={{background:'var(--bg-2)',border:'1px solid var(--line)',borderRadius:8,overflow:'visible',boxShadow:'0 12px 40px rgba(0,0,0,0.4)'}}>
            <div className="p-head">
              <h3>Outputs</h3>
              <div className="p-actions"><span className="mono" style={{fontSize:10.5,color:'var(--fg-4)'}}>2 ports</span></div>
            </div>
            <div style={{padding:'12px 14px',display:'flex',flexDirection:'column',gap:10}}>
              <OutputPortV3 portName="diff" portType="markdown"/>
              <OutputPortV3 portName="screenshots" portType="image_list"/>
              <button className="btn ghost sm" style={{alignSelf:'flex-start'}}><Ic.PlusSm/> Add output port</button>
            </div>
          </div>
          <div style={{display:'grid',gridTemplateColumns:'1fr 1fr 1fr',gap:12,marginTop:28}}>
            {[
              {t:'markdown', d:'Renders .md file with frontmatter schema validation. Inline images referenced in content are rendered in the viewer.'},
              {t:'image',    d:'Single image port. Renders in the viewer at comfortable size with prev/next for repeated ports.'},
              {t:'image_list', d:'Gallery of images from the output directory. Ordered alphabetically, each thumbnail clickable to zoom.'},
            ].map(x => (
              <div key={x.t} className="legend-card pdo">
                <div className="lc-head mono">{x.t}</div>
                <div className="lc-body">{x.d}</div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

// S12 — Library pipeline with drift indicator
function ScreenV3_S12() {
  return (
    <div className="artboard-host" data-screen-label="12 Library drift indicator">
      <Frame mode="run" breadcrumb="No run selected" awaiting={0} activeRuns={0}>
        <div className="panel panel-l">
          <PanelHead title="Library" actions={<button className="btn sm"><Ic.PlusSm/> New</button>}/>
          <div className="lp-tabs">
            <button className="lp-tab"><Ic.Play/> Runs</button>
            <button className="lp-tab on"><Ic.Bookmark/> Library</button>
          </div>
          <div className="p-body" style={{overflow:'visible'}}>
            <div className="lib-section">
              <div className="lib-section-head">
                <span className="chev"><Ic.Chevron/></span>
                Pipeline templates
                <span className="count">· 5</span>
                <span className="spacer"/>
                <button className="lib-add"><Ic.PlusSm/></button>
              </div>
              <div className="lib-list">
                {/* Drifted pipeline with open popover */}
                <div className="lib-row selected" style={{background:'var(--bg-3)',boxShadow:'inset 2px 0 0 var(--acc)',position:'relative'}}>
                  <div className="lib-star-wrap">
                    <button className="ih-star synced" style={{color:'var(--acc)',position:'relative'}}>
                      <Ic.StarFill/>
                      <span className="lib-drift-dot"/>
                    </button>
                    <div className="drift-pop">
                      <div className="dp-head">
                        <span style={{position:'relative',display:'inline-flex'}}>
                          <Ic.StarFill style={{color:'var(--acc)'}}/>
                          <span className="lib-drift-dot"/>
                        </span>
                        <span className="dp-title">Repo copy has changed</span>
                      </div>
                      <div className="dp-desc">
                        Promoted from <span className="mono" style={{color:'var(--fg-2)'}}>/code/myapp/.pdo/pipelines/simple-bugfix.yaml</span>.
                        The repo version has since been modified — 2 commits ahead.
                      </div>
                      <div className="dp-act">
                        <Ic.Refresh style={{width:13,height:13}}/>
                        <div>
                          <b>Update from repo</b>
                          <span className="dpa-sub">Replace library copy with the current repo version.</span>
                        </div>
                      </div>
                      <div className="dp-act">
                        <Ic.Floppy style={{width:13,height:13}}/>
                        <div>
                          <b>Keep library version</b>
                          <span className="dpa-sub">Dismiss the drift indicator; keep as-is.</span>
                        </div>
                      </div>
                      <div className="dp-act danger">
                        <Ic.Trash style={{width:13,height:13}}/>
                        <div>
                          <b>Remove from library</b>
                          <span className="dpa-sub">Pipeline stays in the repo; library entry deleted.</span>
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="lib-row-main">
                    <div className="lib-row-name">simple-bugfix</div>
                    <div className="lib-row-sub">4 nodes · 1 d ago · <span style={{color:'var(--st-await)'}}>2 commits drift</span></div>
                  </div>
                  <button className="icon-btn" style={{width:22,height:22}}><Ic.Kebab/></button>
                </div>
                {/* Other pipelines */}
                {[
                  {id:'feature-with-review', nodes:5, mod:'2 d ago', star:true},
                  {id:'bug-triage',          nodes:4, mod:'5 d ago', star:true},
                  {id:'doc-refresh',         nodes:3, mod:'1 wk ago', star:false},
                  {id:'security-audit',      nodes:6, mod:'2 wk ago', star:true},
                ].map(p => (
                  <div key={p.id} className="lib-row">
                    <button className={"ih-star " + (p.star?'synced':'outline')}>
                      {p.star ? <Ic.StarFill/> : <Ic.Star/>}
                    </button>
                    <div className="lib-row-main">
                      <div className="lib-row-name">{p.id}</div>
                      <div className="lib-row-sub">{p.nodes} nodes · {p.mod}</div>
                    </div>
                    <button className="icon-btn" style={{width:22,height:22}}><Ic.Kebab/></button>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
        <div className="panel panel-c">
          <div className="dag-canvas">
            <div className="empty">
              <div className="emp-art"><Ic.Bookmark/></div>
              <div className="emp-title">Library</div>
              <div className="emp-sub">Select a pipeline template to preview its canvas. Favoriting a repo-scoped pipeline promotes a copy to the global library.</div>
            </div>
          </div>
        </div>
        <div className="panel panel-r">
          <PanelHead title="Inspector"/>
          <div className="p-body"><div className="empty" style={{padding:'40px 24px'}}><div className="emp-sub" style={{maxWidth:240}}>Select a pipeline to inspect.</div></div></div>
        </div>
      </Frame>
    </div>
  );
}

Object.assign(window, {
  ScreenV3_A1, ScreenV3_A2, ScreenV3_A3, ScreenV3_A4,
  ScreenV3_B5, ScreenV3_B6, ScreenV3_B7,
  ScreenV3_C8, ScreenV3_C9, ScreenV3_C10,
  ScreenV3_S11, ScreenV3_S12,
});
