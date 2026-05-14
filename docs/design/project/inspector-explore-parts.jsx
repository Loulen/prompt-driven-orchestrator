// inspector-explore-parts.jsx — components for inspector port row, output card,
// schema field row, and Switch branch editor card.

const SIDES = ['top','left','right','bottom'];

function SidePicker({ value = 'left' }) {
  return (
    <div className="ip-sp" role="group" aria-label="port side">
      <button className={"sp-top "    + (value==='top'    ? 'on':'')} aria-label="top">↑</button>
      <button className={"sp-left "   + (value==='left'   ? 'on':'')} aria-label="left">←</button>
      <button className={"sp-right "  + (value==='right'  ? 'on':'')} aria-label="right">→</button>
      <button className={"sp-bottom " + (value==='bottom' ? 'on':'')} aria-label="bottom">↓</button>
    </div>
  );
}

function RepToggle({ on = false }) {
  return (
    <span className={"rep-toggle " + (on ? 'on' : '')} title="repeated">
      <span className="knob"/>
      <span>rep</span>
    </span>
  );
}

function PortRow({
  name = 'in', side = 'left', dir = 'in',
  repeated = false, hover = false, focused = false, alwaysShow = false,
}) {
  return (
    <div className={"ip" + (hover ? ' is-hover' : '') + (alwaysShow ? ' always-show' : '')}
         data-side={side}>
      <span className="ip-dot"/>
      <input className="ip-name" defaultValue={name}
             autoFocus={focused}/>
      <SidePicker value={side}/>
      <RepToggle on={repeated}/>
      <button className="ip-del" aria-label="delete">
        <Ic.Trash/>
      </button>
    </div>
  );
}

// Two-line variant used for hypothesis I4
function PortRowStacked({
  name = 'in', side = 'left', repeated = false, hover = false, alwaysShow = false,
}) {
  return (
    <div className={"ip" + (hover ? ' is-hover' : '') + (alwaysShow ? ' always-show' : '')}
         data-side={side}>
      <div className="ip-top">
        <span className="ip-dot"/>
        <input className="ip-name" defaultValue={name}/>
        <button className="ip-del" aria-label="delete"><Ic.Trash/></button>
      </div>
      <div className="ip-bot">
        <SidePicker value={side}/>
        <RepToggle on={repeated}/>
      </div>
    </div>
  );
}

/* ======================== Schema field row ======================== */

const FIELD_TYPES = ['string','int','bool','list','enum'];

function FieldChip({ value, hover = false }) {
  return (
    <span className={"fr-chip" + (hover ? ' is-hover' : '')}>
      <span>{value}</span>
      <button className="fr-chip-x" aria-label={"remove " + value}>
        <Ic.X/>
      </button>
    </span>
  );
}

function FieldRow({
  name = 'summary', type = 'string',
  allowed = [], chipHoverIdx = -1, inputValue = '',
}) {
  return (
    <div className="fr">
      <input className="fr-name" defaultValue={name}/>
      <select className="fr-type" defaultValue={type}>
        {FIELD_TYPES.map(t => <option key={t} value={t}>{t}</option>)}
      </select>
      <button className="fr-del" aria-label="delete field"><Ic.Trash/></button>

      {type === 'enum' && (
        <div className="fr-enum-body">
          <div className="fr-enum-label mono">Allowed values</div>
          {allowed.length === 0 ? (
            <div className="fr-empty">no allowed values yet — add one to constrain this field</div>
          ) : (
            <div className="fr-enum-chips">
              {allowed.map((v, i) => <FieldChip key={v} value={v} hover={i === chipHoverIdx}/>)}
            </div>
          )}
          <div className="fr-add-row">
            <input placeholder="new value" defaultValue={inputValue}/>
            <button className="fr-add-btn" disabled={!inputValue}>
              <Ic.Plus/> Add
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/* ======================== Output port card ======================== */

function OutputCard({
  name = 'diff',
  side = 'right',
  collapsed = false,
  fields = [{ name: 'summary', type: 'string' }],
  showAddField = true,
  chipHoverIdx = -1,
}) {
  return (
    <div className="op">
      <div className={"op-head" + (collapsed ? ' is-collapsed' : '')}>
        <button className="chev-btn" aria-label="collapse"><Ic.Chevron/></button>
        <input className="op-name" defaultValue={name}/>
        <div className="op-actions">
          <SidePicker value={side}/>
          <RepToggle on={false}/>
          <button className="op-del" aria-label="delete"><Ic.Trash/></button>
        </div>
      </div>
      {!collapsed && (
        <div className="op-body">
          {fields.length === 0 && <div className="op-empty">no fields declared yet</div>}
          {fields.map((f, i) => (
            <FieldRow key={i}
              name={f.name} type={f.type}
              allowed={f.allowed || []}
              chipHoverIdx={f.name === 'verdict' ? chipHoverIdx : -1}
              inputValue={f.inputValue || ''}/>
          ))}
          {showAddField && (
            <button className="op-add-field">
              <Ic.Plus/> Add field
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/* ======================== Switch branch card ======================== */

const OPS = ['eq','neq','lt','lte','gt','gte','in','not_in'];

function CondRow({ field = 'verdict', op = 'eq', value = 'PASS',
                   valueKind = 'enum', enumOpts = ['PASS','FAIL','NEEDS_WORK'],
                   empty = false }) {
  return (
    <div className="sb-cond">
      <select className="sel" defaultValue={field}>
        <option value={field}>{field}</option>
        <option>complexity_score</option>
        <option>files_changed</option>
      </select>
      <select className="sel" defaultValue={op}>
        {OPS.map(o => <option key={o}>{o}</option>)}
      </select>
      {valueKind === 'enum' ? (
        <select className={"sel" + (empty ? ' empty' : '')} defaultValue={empty ? '' : value}>
          {empty && <option value="">choose value…</option>}
          {enumOpts.map(o => <option key={o}>{o}</option>)}
        </select>
      ) : (
        <input className={"input" + (empty ? ' empty' : '')}
               placeholder="value"
               defaultValue={empty ? '' : value}/>
      )}
      <button className="sb-del-c" aria-label="delete condition"><Ic.X/></button>
    </div>
  );
}

function GripIcon() {
  return (
    <svg width="10" height="14" viewBox="0 0 10 14" fill="currentColor" aria-hidden="true">
      <circle cx="3" cy="3" r="1"/><circle cx="7" cy="3" r="1"/>
      <circle cx="3" cy="7" r="1"/><circle cx="7" cy="7" r="1"/>
      <circle cx="3" cy="11" r="1"/><circle cx="7" cy="11" r="1"/>
    </svg>
  );
}

function SwitchBranch({
  name = 'pass', conditions = [], isFirst = false, isLast = false,
  isDefault = false,
}) {
  if (isDefault) {
    return (
      <div className="sb is-default">
        <div className="sb-head">
          <input className="sb-name" defaultValue="default"/>
          <span className="sb-pin">pinned · catch-all</span>
        </div>
        <div className="sb-body">
          <span className="catchall">
            <Ic.ArrowRight/> taken when no branch above matches
          </span>
        </div>
      </div>
    );
  }
  const multi = conditions.length > 1;
  return (
    <div className="sb">
      <div className="sb-head">
        <span className="sb-grip" aria-label="drag to reorder">
          <GripIcon/>
        </span>
        <input className="sb-name" defaultValue={name}/>
        <div className="sb-arrows">
          <button aria-label="move up" disabled={isFirst}><Ic.Chevron style={{transform:'rotate(180deg)'}}/></button>
          <button aria-label="move down" disabled={isLast}><Ic.Chevron/></button>
        </div>
        <button className="sb-del" aria-label="delete branch"><Ic.Trash/></button>
      </div>
      <div className="sb-body">
        <div className={"sb-cond-list " + (multi ? 'multi' : 'single')}>
          {conditions.map((c, i) => <CondRow key={i} {...c}/>)}
        </div>
        <button className="sb-add-c">
          <Ic.Plus/> Add condition
        </button>
      </div>
    </div>
  );
}

window.PortRow = PortRow;
window.PortRowStacked = PortRowStacked;
window.SidePicker = SidePicker;
window.RepToggle = RepToggle;
window.FieldRow = FieldRow;
window.FieldChip = FieldChip;
window.OutputCard = OutputCard;
window.SwitchBranch = SwitchBranch;
window.CondRow = CondRow;
