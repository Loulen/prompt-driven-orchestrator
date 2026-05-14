// inspector-explore-app.jsx — sheet layout and renderings.

function Cell({ caption, edTag, children, className = '' }) {
  return (
    <div className={"cell " + className}>
      {edTag && <span className="ed-tag">{edTag}</span>}
      {caption && <div className="ed-cap mono">{caption}</div>}
      {children}
    </div>
  );
}

function Sheet({ num, title, desc, children }) {
  return (
    <section className="sheet" id={"sheet-" + num}>
      <div className="sheet-head">
        <span className="sheet-num mono">SHEET {String(num).padStart(2,'0')}</span>
        <h2 className="sheet-title">{title}</h2>
      </div>
      {desc && <p className="sheet-desc">{desc}</p>}
      {children}
    </section>
  );
}

function HypBlock({ tag, title, caption, className = '', children }) {
  return (
    <div className={"hyp-block " + className}>
      <div className="hb-head">
        <span className="hb-tag mono">{tag}</span>
        <span className="hb-title">{title}</span>
        <span className="hb-cap">{caption}</span>
      </div>
      <div className="hb-body">{children}</div>
    </div>
  );
}

/* =====================================================================
   SHEET 1 — Inspector port row, 4 hypotheses × 5 cells
   ===================================================================== */

function Sheet1() {
  // 5 cells per hypothesis:
  //  1. resting · single input
  //  2. hovered · controls revealed
  //  3. focused · name editing
  //  4. multi-port list (3 rows)
  //  5. side-mix · same node, ports on different sides

  const hyps = [
    { tag: 'I1', cls: 'i1', title: 'Hairline',
      caption: 'No card. Rows separated by a faint divider; controls only on hover. Densest, sober.' },
    { tag: 'I2', cls: 'i2', title: 'Inset card',
      caption: 'Each port gets a low-elevation card. More structure, easier scanning.' },
    { tag: 'I3', cls: 'i3', title: 'Side-anchored',
      caption: 'Left rail spells out the chosen side; mirrors the placement visually.' },
    { tag: 'I4', cls: 'i4', title: 'Two-line',
      caption: 'Name on top, side + repeated below. Best when names are long.' },
  ];

  const renderCells = (cls) => {
    const Row = cls === 'i4' ? PortRowStacked : PortRow;
    return (
      <div className={"sheet1-grid " + cls}>
        <Cell edTag="01" caption="resting · single input">
          <Row name="in" side="left" dir="in"/>
        </Cell>
        <Cell edTag="02" caption="hovered · controls revealed">
          <Row name="in" side="left" dir="in" hover/>
        </Cell>
        <Cell edTag="03" caption="focused · renaming">
          <Row name="in" side="left" dir="in" hover focused/>
        </Cell>
        <div className="cell-pair">
          <Cell edTag="04" caption="multi-port list">
            <Row name="in"    side="left"  dir="in"  alwaysShow/>
            <Row name="break" side="top"   dir="in"  alwaysShow/>
            <Row name="body"  side="right" dir="out" alwaysShow/>
            <Row name="done"  side="right" dir="out" repeated alwaysShow/>
          </Cell>
          <Cell edTag="05" caption="side-mix · 4 ports across the 4 sides">
            <Row name="config" side="top"    dir="in"  alwaysShow/>
            <Row name="input"  side="left"   dir="in"  alwaysShow/>
            <Row name="result" side="right"  dir="out" alwaysShow/>
            <Row name="trace"  side="bottom" dir="out" alwaysShow/>
          </Cell>
        </div>
      </div>
    );
  };

  return (
    <Sheet num={1}
      title="Inspector · port row"
      desc="Standalone control for a single port in the inspector — dot · name · side picker · repeated · delete. Hover reveals secondary controls (delete, repeated chip). Side picker mirrors the dag side semantics from the canvas redesign. Drag-reorder grip not shown here; this is the row body only.">
      {hyps.map(h => (
        <HypBlock key={h.tag} tag={h.tag} title={h.title} caption={h.caption} className={h.cls}>
          {renderCells(h.cls)}
        </HypBlock>
      ))}
    </Sheet>
  );
}

/* =====================================================================
   SHEET 2 — Output port card, 3 hypotheses × 6 cells
   ===================================================================== */

function Sheet2() {
  const hyps = [
    { tag: 'O1', cls: 'o1', title: 'Bordered card',
      caption: 'Dark head, lighter body. Clear container, scannable in a long list.' },
    { tag: 'O2', cls: 'o2', title: 'Side spine',
      caption: 'Thin left bar. Quieter container — fewer hairlines on screen.' },
    { tag: 'O3', cls: 'o3', title: 'Tab head',
      caption: 'Head outdents like a folder tab; body anchors below. Strongest header reading order.' },
  ];

  const baseFields = [
    { name: 'summary', type: 'string' },
    { name: 'files_changed', type: 'int' },
  ];
  const enumFields = [
    { name: 'summary', type: 'string' },
    {
      name: 'verdict', type: 'enum',
      allowed: ['PASS', 'FAIL', 'NEEDS_WORK'],
      inputValue: '',
    },
  ];
  const enumEditingFields = [
    { name: 'summary', type: 'string' },
    {
      name: 'verdict', type: 'enum',
      allowed: ['PASS', 'FAIL', 'NEEDS_WORK'],
      inputValue: 'NEEDS_HUMAN',
    },
  ];

  const renderCells = (cls) => (
    <div className={"sheet2-grid " + cls}>
      <Cell edTag="01" caption="collapsed · just header">
        <OutputCard name="diff" side="right" collapsed fields={[]}/>
      </Cell>
      <Cell edTag="02" caption="empty · no fields yet">
        <OutputCard name="diff" side="right" fields={[]}/>
      </Cell>
      <Cell edTag="03" caption="2 fields · scalars">
        <OutputCard name="diff" side="right" fields={baseFields}/>
      </Cell>
      <Cell edTag="04" className="tall" caption="enum field · 3 allowed values">
        <OutputCard name="diff" side="right" fields={enumFields}/>
      </Cell>
      <Cell edTag="05" className="tall" caption="adding 4th enum value · chip hover removes #2">
        <OutputCard name="diff" side="right" fields={enumEditingFields} chipHoverIdx={1}/>
      </Cell>
      <Cell edTag="06" caption="side-mix · output on top edge">
        <OutputCard name="break_signal" side="top" fields={[{name:'reason', type:'string'}]}/>
      </Cell>
    </div>
  );

  return (
    <Sheet num={2}
      title="Inspector · output port card"
      desc="Full card for one output: collapsible head with name + side + repeated + delete; body lists declared schema fields with inline add. Enum fields expand inside the row to manage their allowed values via chips. Card is the unit that gets drag-reordered inside the inspector.">
      {hyps.map(h => (
        <HypBlock key={h.tag} tag={h.tag} title={h.title} caption={h.caption} className={h.cls}>
          {renderCells(h.cls)}
        </HypBlock>
      ))}
    </Sheet>
  );
}

/* =====================================================================
   SHEET 3 — Schema field row, type-driven shapes (1 hypothesis × 6 cells)
   ===================================================================== */

function Sheet3() {
  return (
    <Sheet num={3}
      title="Output schema · field row"
      desc="Field row inside an output card. Type drives the row footprint: scalars stay single-line; enum expands a chip editor inline. The same shape is reusable inside Start’s output and inside any other node’s output cards.">
      <HypBlock tag="F1" title="Type-driven row" caption="One row template that expands when type === enum. No second card.">
        <div className="grid-row row-3">
          <Cell edTag="01" caption="string · scalar">
            <FieldRow name="summary" type="string"/>
          </Cell>
          <Cell edTag="02" caption="int · scalar">
            <FieldRow name="files_changed" type="int"/>
          </Cell>
          <Cell edTag="03" caption="bool · scalar">
            <FieldRow name="is_clean" type="bool"/>
          </Cell>
          <Cell edTag="04" caption="list · scalar"  className="">
            <FieldRow name="touched_paths" type="list"/>
          </Cell>
          <Cell edTag="05" caption="enum · empty allowed-values">
            <FieldRow name="verdict" type="enum" allowed={[]} inputValue=""/>
          </Cell>
          <Cell edTag="06" caption="enum · 3 values · adding a 4th">
            <FieldRow name="verdict" type="enum"
                      allowed={['PASS','FAIL','NEEDS_WORK']}
                      inputValue="NEEDS_HUMAN"/>
          </Cell>
        </div>
      </HypBlock>
    </Sheet>
  );
}

/* =====================================================================
   SHEET 4 — Switch branch editor card, 3 hypotheses × 5 cells each
   ===================================================================== */

function Sheet4() {
  const hyps = [
    { tag: 'S1', cls: 's1', title: 'AND rail',
      caption: 'Vertical rule on the left of the condition list; sideways AND label.' },
    { tag: 'S2', cls: 's2', title: 'AND chip between',
      caption: 'Inline chip between every two rows. Most explicit.' },
    { tag: 'S3', cls: 's3', title: 'Bracket all-of',
      caption: 'A square bracket groups all conditions under one "all" label.' },
  ];

  const renderCells = (cls) => (
    <div className={"sheet4-grid " + cls}>
      <Cell edTag="01" caption="branch · single condition">
        <SwitchBranch name="pass" conditions={[
          { field:'verdict', op:'eq', value:'PASS', valueKind:'enum',
            enumOpts:['PASS','FAIL','NEEDS_WORK'] },
        ]} isFirst/>
      </Cell>
      <Cell edTag="02" caption="branch · 2 conditions (AND)">
        <SwitchBranch name="needs_human" conditions={[
          { field:'verdict', op:'eq', value:'NEEDS_WORK', valueKind:'enum',
            enumOpts:['PASS','FAIL','NEEDS_WORK'] },
          { field:'complexity_score', op:'gte', value:'7', valueKind:'string' },
        ]}/>
      </Cell>
      <Cell edTag="03" className="tall" caption="branch · 3 conditions, empty value mid-edit">
        <SwitchBranch name="rework" conditions={[
          { field:'verdict', op:'eq', value:'FAIL', valueKind:'enum',
            enumOpts:['PASS','FAIL','NEEDS_WORK'] },
          { field:'files_changed', op:'lt', value:'10', valueKind:'string' },
          { field:'complexity_score', op:'gte', empty: true, valueKind:'string' },
        ]}/>
      </Cell>
      <Cell edTag="04" caption="default branch · always last, pinned">
        <SwitchBranch isDefault/>
      </Cell>
      <Cell edTag="05" caption="ordered list · 3 branches + default + add">
        <div className="sb-list">
          <SwitchBranch name="pass" conditions={[
            { field:'verdict', op:'eq', value:'PASS', valueKind:'enum',
              enumOpts:['PASS','FAIL','NEEDS_WORK'] },
          ]} isFirst/>
          <SwitchBranch name="needs_human" conditions={[
            { field:'verdict', op:'eq', value:'NEEDS_WORK', valueKind:'enum',
              enumOpts:['PASS','FAIL','NEEDS_WORK'] },
            { field:'complexity_score', op:'gte', value:'7', valueKind:'string' },
          ]}/>
          <SwitchBranch name="rework" conditions={[
            { field:'verdict', op:'eq', value:'FAIL', valueKind:'enum',
              enumOpts:['PASS','FAIL','NEEDS_WORK'] },
          ]} isLast/>
          <SwitchBranch isDefault/>
          <button className="add-branch">
            <Ic.Plus/> Add branch
          </button>
        </div>
      </Cell>
    </div>
  );

  return (
    <Sheet num={4}
      title="Switch · branch editor card"
      desc="Inspector card for a single named branch on a Switch node. Header carries grip / name / reorder arrows / delete; body lists conditions ANDed together. The 'default' branch is pinned at the bottom, can be renamed but not reordered, and has no condition body. Conditions are sequential AND — OR semantics are achieved by adding another named branch above the catch-all.">
      {hyps.map(h => (
        <HypBlock key={h.tag} tag={h.tag} title={h.title} caption={h.caption} className={h.cls}>
          {renderCells(h.cls)}
        </HypBlock>
      ))}
    </Sheet>
  );
}

/* =====================================================================
   SHEET 5 — Cohesion check (no choice yet — paired sketches)
   ===================================================================== */

function Sheet5() {
  return (
    <Sheet num={5}
      title="Cohesion check · port row + output card + Switch branch"
      desc="If the user picks I2 (inset card) + O1 (bordered) + S1 (AND rail), the three sit next to each other in the same inspector with consistent border, radius, head treatment, and density. This sheet is a sanity preview, not a recommendation — swap in any combination once a winner is chosen.">

      <div className="cohesion-pair">
        <div className="ch-col">
          <span className="ch-tag mono">A · generic node (1 in, 1 out · diff)</span>
          <div className="i2">
            <PortRow name="in" side="left" dir="in" alwaysShow/>
          </div>
          <div className="o1">
            <OutputCard name="diff" side="right" fields={[
              { name: 'summary', type: 'string' },
              { name: 'verdict', type: 'enum',
                allowed: ['PASS','FAIL','NEEDS_WORK'] },
            ]}/>
          </div>
        </div>

        <div className="ch-col">
          <span className="ch-tag mono">B · Switch node (1 in · 3 named branches + default)</span>
          <div className="i2">
            <PortRow name="in" side="left" dir="in" alwaysShow/>
          </div>
          <div className="s1 sb-list">
            <SwitchBranch name="pass" conditions={[
              { field:'verdict', op:'eq', value:'PASS', valueKind:'enum',
                enumOpts:['PASS','FAIL','NEEDS_WORK'] },
            ]} isFirst/>
            <SwitchBranch name="needs_human" conditions={[
              { field:'verdict', op:'eq', value:'NEEDS_WORK', valueKind:'enum',
                enumOpts:['PASS','FAIL','NEEDS_WORK'] },
              { field:'complexity_score', op:'gte', value:'7', valueKind:'string' },
            ]}/>
            <SwitchBranch name="rework" conditions={[
              { field:'verdict', op:'eq', value:'FAIL', valueKind:'enum',
                enumOpts:['PASS','FAIL','NEEDS_WORK'] },
            ]} isLast/>
            <SwitchBranch isDefault/>
          </div>
        </div>

      </div>
    </Sheet>
  );
}

/* =====================================================================
   App shell
   ===================================================================== */

function App() {
  return (
    <main className="expl-doc">
      <div className="expl-head">
        <div>
          <div className="eyebrow mono">Maestro · inspector iteration</div>
          <h1>Port row / output card / Switch branch · component sheets</h1>
        </div>
        <div className="sub">
          Five sheets of component variations. Each hypothesis is one self-consistent
          treatment applied across every state. Pick column-by-column; the integration
          pass merges the winners back into the inspector.
        </div>
      </div>

      <nav className="toc mono">
        <a href="#sheet-1"><span className="ord">01</span>Port row · I1–I4</a>
        <a href="#sheet-2"><span className="ord">02</span>Output card · O1–O3</a>
        <a href="#sheet-3"><span className="ord">03</span>Field row · type-driven</a>
        <a href="#sheet-4"><span className="ord">04</span>Switch branch · S1–S3</a>
        <a href="#sheet-5"><span className="ord">05</span>Cohesion check</a>
      </nav>

      <Sheet1/>
      <Sheet2/>
      <Sheet3/>
      <Sheet4/>
      <Sheet5/>

      <footer className="outro">
        <div>
          <h5>Out of scope here</h5>
          <ul>
            <li>Canvas / node card visuals (covered in the prior sheet).</li>
            <li>Drag wire feedback, drop tolerance halo (port row on the canvas).</li>
            <li>Inspector chrome — section headers, scroll, sticky save bar.</li>
          </ul>
        </div>
        <div>
          <h5>Next step</h5>
          <ul>
            <li>Pick a port row hypothesis (I1–I4).</li>
            <li>Pick an output card hypothesis (O1–O3).</li>
            <li>Pick a Switch branch hypothesis (S1–S3).</li>
            <li>Confirm field-row shape (F1) or call out a desired alternative.</li>
          </ul>
        </div>
      </footer>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App/>);
