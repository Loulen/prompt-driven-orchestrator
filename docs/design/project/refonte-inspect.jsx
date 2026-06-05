// refonte-inspect.jsx — detail panels for the refonte
//   EdgeDetailPanel  — author when: (field/op/value, boolean toggle, iter field),
//                       else flag, routing reset, runtime trigger status (panel-only)
//   NodeInspectorRf  — id, role/prompt editor, output schemas, pooled inputs
//   RegionInspector  — bounded (max-iter) and collection (source field + members)

// ── Edge detail panel ───────────────────────────────────────────────────────
function EdgeDetailPanel({
  route = { from: 'Reviewer', out: 'verdict', to: 'Implementer' },
  field = 'is_blocking', fieldType = 'bool', op = '=', value = 'true',
  fieldMenuOpen = true, isElse = false, region = 'review-loop',
  trigger = { fired: true, lastValue: 'true', evaluatedAt: '14:42:06', iter: 2 },
}) {
  const FIELDS = [
    { name: 'verdict', type: 'enum', vals: 'PASS · FAIL · NEEDS_WORK' },
    { name: 'is_blocking', type: 'bool' },
    { name: 'files_changed', type: 'int' },
    { name: 'severity', type: 'enum', vals: 'low · med · high' },
    { name: 'iter', type: 'counter', region: true },
  ];
  const isBool = fieldType === 'bool';
  return (
    <>
      <div className="rf-edge-head">
        <span className="reh-glyph"><Ic.ArrowRight/></span>
        <div style={{ minWidth: 0 }}>
          <div className="rf-edge-route">
            <span className="ern-node">{route.from}</span>
            <span className="ern-mid">.{route.out}</span>
            <Ic.ArrowRight style={{ width: 11, height: 11, color: 'var(--fg-4)' }}/>
            <span className="ern-node">{route.to}</span>
          </div>
          <div className="rf-edge-sub">edge · conditional route</div>
        </div>
      </div>

      <div className="p-body">
        <div className="p-sect">
          <SectionHead title="When"/>
          <div className="field">
            <label>Condition</label>
            <div className={"rf-when" + (isBool ? ' bool' : '')}>
              <div className="rf-when-field-wrap">
                <input className="input mono" value={field} readOnly
                  style={{ borderColor: 'var(--acc)', boxShadow: '0 0 0 1px var(--acc)' }}/>
                {fieldMenuOpen && (
                  <div className="rf-field-menu">
                    {FIELDS.map(f => (
                      <div key={f.name} className={"rf-field-opt" + (f.name === field ? ' on' : '')}>
                        <span className={f.region ? 'fo-iter' : ''}>{f.name}</span>
                        <span className="fo-type">{f.region ? 'region counter' : f.vals || f.type}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
              <select className="select mono" defaultValue={op}>
                <option>=</option><option>≠</option><option>&gt;</option>
                <option>≥</option><option>&lt;</option><option>≤</option>
              </select>
              {!isBool && <input className="input mono" defaultValue={value}/>}
              {isBool && (
                <div className="rf-bool">
                  <button className={value === 'true' ? 'on' : ''}>true</button>
                  <button className={value === 'false' ? 'on' : ''}>false</button>
                </div>
              )}
            </div>
            <div className="help" style={{ marginTop: 6 }}>
              Boolean field <span className="mono" style={{ color: 'var(--fg-3)' }}>{field}</span> — value is a true/false toggle, no text box.
            </div>
          </div>

          <div className="rf-else-row">
            <span className="toggle" style={{ background: isElse ? 'var(--acc)' : 'var(--bg-5)' }}/>
            <div className="er-txt">
              <div className="er-name">Treat as <span className="mono">else</span></div>
              <div className="er-help">Fires only if no sibling edge from this output matched.</div>
            </div>
          </div>
        </div>

        <div className="p-sect">
          <SectionHead title="Available fields"/>
          <div className="help" style={{ lineHeight: 1.6 }}>
            Output schema of <span className="mono" style={{ color: 'var(--fg-3)' }}>{route.from}.{route.out}</span>,
            plus <span className="mono" style={{ color: '#c2a15e' }}>iter</span> — the counter of the enclosing
            region <span className="mono" style={{ color: 'var(--fg-3)' }}>{region}</span>.
          </div>
          <div className="rf-pill" style={{ position: 'static', transform: 'none', marginTop: 10, display: 'inline-flex' }}>
            <span className="iter">iter</span><span className="pop">≥</span><span className="pv">max</span>
          </div>
          <span style={{ fontSize: 10.5, color: 'var(--fg-4)', marginLeft: 8 }}>example exhaust-exit edge</span>
        </div>

        <div className="p-sect">
          <SectionHead title="Routing"/>
          <div className="row-h">
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span className="rf-insp-id"><span style={{ width: 6, height: 6, borderRadius: 3, background: 'var(--acc)' }}/> manually pinned</span>
            </div>
            <button className="btn sm ghost"><Ic.Refresh style={{ width: 11, height: 11 }}/> Re-route automatically</button>
          </div>
        </div>

        <div className="p-sect" style={{ borderBottom: 'none' }}>
          <SectionHead title="Runtime"/>
          <div className="rf-trigger">
            <div className="tr-head"><Ic.Pulse style={{ width: 12, height: 12 }}/> trigger status · this run</div>
            <div className="tr-row">
              <span className={"tr-dot " + (trigger.fired ? 'fired' : 'not')}/>
              {trigger.fired ? 'fired' : 'not fired'}
              <span className={"tr-val" + (trigger.fired ? ' fired' : '')}>{trigger.fired ? '✓ matched' : '— skipped'}</span>
            </div>
            <div className="tr-row">last value<span className="tr-val">{field} = {trigger.lastValue}</span></div>
            <div className="tr-row">evaluated<span className="tr-val">iter {trigger.iter} · {trigger.evaluatedAt}</span></div>
          </div>
          <div className="help" style={{ marginTop: 8 }}>
            Trigger status lives here only — the canvas never shows fired / not-fired on edges.
          </div>
        </div>
      </div>
    </>
  );
}

// ── Node inspector (pooling legibility) ─────────────────────────────────────
function NodeInspectorRf({
  name = 'Implementer', nid = '9k2x7m', mark = 'code',
  outputs = [
    { name: 'diff', type: 'markdown', fields: [['summary', 'string'], ['files_changed', 'int'], ['verdict', 'enum', 'PASS · FAIL']] },
  ],
  inputs = [
    { name: 'review', pooled: ['security-reviewer', 'perf-reviewer'] },
    { name: 'plan', pooled: ['planner'] },
  ],
}) {
  return (
    <>
      <PanelHead title={name} actions={<button className="icon-btn"><Ic.X/></button>}/>
      <div className="p-body">
        <div className="p-sect" style={{ paddingBottom: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 9, marginBottom: 8 }}>
            <span className="rf-ntype"><RfRole role="agent"/></span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 600 }}>{name}</div>
              <div style={{ marginTop: 4 }}>
                <span className="rf-insp-id"><Ic.Copy style={{ width: 10, height: 10 }}/> {nid}</span>
              </div>
            </div>
            <span className={"rf-mark " + mark} style={{ width: 22, height: 22 }}>{mark === 'code' ? <Ic.Code/> : <Ic.Doc/>}</span>
            <button className="ih-star outline"><Ic.Star/></button>
          </div>
          <div className="help">Node ID lives in the inspector — the canvas card shows only icon, name and the code/doc marker.</div>
        </div>

        <div className="p-sect">
          <SectionHead title="Role"/>
          <textarea className="textarea mono" rows={4} defaultValue={"You are an implementation agent. Read `plan` and `review`, apply the smallest change that satisfies the plan, then write a unified diff to `diff` with a verdict in the frontmatter."}/>
          <div className="rf-role-tools">
            {['read_file', 'edit_file', 'bash', 'git'].map(t => (
              <span key={t} className="rf-tool-chip"><Ic.Code/>{t}</span>
            ))}
          </div>
        </div>

        <div className="p-sect">
          <SectionHead title="Outputs" count={outputs.length}/>
          {outputs.map(o => (
            <div key={o.name} className="rf-out-schema">
              <div className="os-head">
                <span className="os-dot"/>
                <span className="os-name">{o.name}</span>
                <span className="os-type">{o.type}</span>
              </div>
              <div className="os-fields">
                {o.fields.map((f, i) => (
                  <div key={i} className="os-field">
                    <span>{f[0]}</span>
                    <span className={f[1] === 'enum' ? 'of-enum' : 'of-t'}>{f[1]}{f[2] ? ` · ${f[2]}` : ''}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div className="p-sect" style={{ borderBottom: 'none' }}>
          <SectionHead title="Inputs" count={inputs.length}/>
          <div className="help" style={{ marginBottom: 8 }}>
            Same-named edges pool into one logical input. The canvas hides this; here the sources are spelled out.
          </div>
          {inputs.map(inp => (
            <div key={inp.name} className={"rf-pool" + (inp.pooled.length === 1 ? ' single' : '')}>
              <div className="pl-top">
                <span className="pl-name">{inp.name}</span>
                <span className="pl-count">{inp.pooled.length} source{inp.pooled.length !== 1 ? 's' : ''}</span>
              </div>
              <div className="pl-srcs">
                {inp.pooled.map(s => (
                  <div key={s} className="pl-src"><span className="ps-arr">←</span><span className="ps-from">{s}</span></div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}

// ── Region inspector (bounded / collection) ─────────────────────────────────
function RegionInspector({ flavor = 'bounded' }) {
  return (
    <>
      <PanelHead title="Loop region" actions={<button className="icon-btn"><Ic.X/></button>}/>
      <div className="p-body">
        <div className="p-sect">
          <div className="rf-flavor-seg">
            <div className={"rf-flavor-card bounded" + (flavor === 'bounded' ? ' on' : '')}>
              <div className="fc-top"><span className="fc-glyph">↻</span> Bounded</div>
              <div className="fc-sub">Sequential counter. Born by auto-detecting a cycle.</div>
            </div>
            <div className={"rf-flavor-card collection" + (flavor === 'collection' ? ' on' : '')}>
              <div className="fc-top"><span className="fc-glyph">⇉</span> Collection</div>
              <div className="fc-sub">Parallel fan-out over a list field.</div>
            </div>
          </div>
        </div>

        {flavor === 'bounded' ? (
          <>
            <div className="p-sect">
              <SectionHead title="Bound"/>
              <div className="field">
                <label>Max iterations</label>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <input className="input mono" defaultValue="5" style={{ width: 80 }}/>
                  <span className="rf-badge bounded" style={{ position: 'static' }}><span style={{ fontFamily: 'var(--font-mono)' }}>↻</span> 2 / 5 this run</span>
                </div>
                <div className="help" style={{ marginTop: 6 }}>Counter increments each cycle. Exposed to edges as <span className="mono" style={{ color: '#c2a15e' }}>iter</span>.</div>
              </div>
              <div className="field" style={{ marginBottom: 0 }}>
                <label>On exhaust (iter ≥ max, no exit matched)</label>
                <div className="help">Region enters an explicit <span style={{ color: 'var(--st-blocked)' }}>blocked</span> state on the canvas — never a silent hang. Routable from the manager panel.</div>
              </div>
            </div>
            <div className="p-sect" style={{ borderBottom: 'none' }}>
              <SectionHead title="Members" count={2}/>
              {[['Implementer', 'code'], ['Reviewer', 'doc']].map(([m, k]) => (
                <div key={m} className="rf-member">
                  <span className="mb-grip">⠿</span>
                  <span className="rf-ntype" style={{ width: 18, height: 18 }}><RfRole/></span>
                  {m}
                  <span className={"rf-mark " + k + " mb-mark"} style={{ width: 16, height: 16 }}>{k === 'code' ? <Ic.Code/> : <Ic.Doc/>}</span>
                </div>
              ))}
            </div>
          </>
        ) : (
          <>
            <div className="p-sect">
              <SectionHead title="Fan out over"/>
              <div className="field">
                <label>Source collection field</label>
                <input className="input mono" defaultValue="triage.issues[]"/>
                <div className="help" style={{ marginTop: 6 }}>Header reads <span className="mono" style={{ color: '#c2a15e' }}>⇉ N items</span> at runtime — one parallel branch per element.</div>
              </div>
              <div className="field" style={{ marginBottom: 0 }}>
                <label>Barrier</label>
                <div className="help">All branches converge into <span className="mono" style={{ color: 'var(--fg-3)' }}>Merge</span> before the pipeline continues.</div>
              </div>
            </div>
            <div className="p-sect" style={{ borderBottom: 'none' }}>
              <SectionHead title="Members" count={1}/>
              <div className="rf-member">
                <span className="mb-grip">⠿</span>
                <span className="rf-ntype" style={{ width: 18, height: 18 }}><RfRole/></span>
                Fixer
                <span className="rf-mark code mb-mark" style={{ width: 16, height: 16 }}><Ic.Code/></span>
              </div>
              <div className="help" style={{ marginTop: 8 }}>Single member renders as a compact <span className="mono" style={{ color: '#c2a15e' }}>⇉ N items</span> badge on the node, not a box.</div>
            </div>
          </>
        )}
      </div>
    </>
  );
}

Object.assign(window, { EdgeDetailPanel, NodeInspectorRf, RegionInspector });
