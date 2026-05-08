// data.js — sample data for Maestro mockup

const RUNS = [
  {
    id: 'run-2026-05-06-1430-a3f',
    pipeline: 'feature-with-review',
    status: 'running',
    title: 'Implement search filter for archived projects',
    when: '4 min ago',
    elapsed: '04:12',
    iter: 2,
    awaiting: false,
  },
  {
    id: 'run-2026-05-06-1109-7c2',
    pipeline: 'feature-with-review',
    status: 'awaiting_user',
    title: 'Migrate auth provider to OAuth2',
    when: '23 min ago',
    elapsed: '17:48',
    iter: 1,
    awaiting: true,
  },
  {
    id: 'run-2026-05-06-0902-d11',
    pipeline: 'bug-triage',
    status: 'blocked',
    title: 'Investigate flaky CI on `main` branch',
    when: '2 h ago',
    elapsed: '32:05',
    iter: 5,
    awaiting: false,
  },
  {
    id: 'run-2026-05-06-0815-9be',
    pipeline: 'doc-refresh',
    status: 'done',
    title: 'Refresh README + CONTRIBUTING after release',
    when: '3 h ago',
    elapsed: '08:22',
  },
  {
    id: 'run-2026-05-05-1740-44e',
    pipeline: 'feature-with-review',
    status: 'done',
    title: 'Add export to CSV in reports view',
    when: '17 h ago',
    elapsed: '12:47',
  },
  {
    id: 'run-2026-05-05-1112-2af',
    pipeline: 'security-audit',
    status: 'failed',
    title: 'Audit deps for CVE-2025-39022',
    when: '21 h ago',
    elapsed: '03:18',
  },
  {
    id: 'run-2026-05-04-2031-6b8',
    pipeline: 'feature-with-review',
    status: 'archived',
    title: 'Spike: switch from yarn to pnpm',
    when: '2 d ago',
    elapsed: '54:01',
  },
];

const PIPELINES = [
  { id: 'feature-with-review', kind: 'repo', nodes: 5, modified: '2 d ago' },
  { id: 'bug-triage', kind: 'repo', nodes: 4, modified: '5 d ago' },
  { id: 'doc-refresh', kind: 'user', nodes: 3, modified: '1 wk ago' },
  { id: 'security-audit', kind: 'user', nodes: 6, modified: '2 wk ago' },
  { id: 'release-notes', kind: 'user', nodes: 3, modified: '3 wk ago' },
];

// Pipeline graph for `feature-with-review`
// Coords are in canvas-px, anchored top-left of the node.
const FWR_NODES = [
  { id: 'plan',        nid: 'k7m2x9',  name: 'Planner',        type: 'doc',  status: 'done',          x:  60,  y: 200, ports: { in: ['issue'], out: ['plan'] } },
  { id: 'impl',        nid: '9k2x7m',  name: 'Implementer',    type: 'code', status: 'running',       x: 320,  y: 100, iter: '2/5', ports: { in: ['plan', 'review_feedback'], out: ['diff'] }, portSides: { review_feedback: 'bottom' } },
  { id: 'review',      nid: 'q4n8jp',  name: 'Reviewer',       type: 'doc',  status: 'running',       x: 320,  y: 320, iter: '2/5', ports: { in: ['diff'], out: ['review_feedback', 'verdict'] }, portSides: { review_feedback: 'top' } },
  { id: 'tests',       nid: 'r3w6tz',  name: 'Tests',          type: 'code', status: 'pending',       x: 600,  y: 100, ports: { in: ['diff'], out: ['result'] } },
  { id: 'merge',       nid: 'h8s1vc',  name: 'Merge',          type: 'code', status: 'pending',       x: 600,  y: 320, ports: { in: ['verdict', 'result'], out: ['branch'] }, portSides: { verdict: 'top' } },
];

const FWR_EDGES = [
  { id: 'e1', from: 'plan',   fromPort: 'plan',     to: 'impl',  toPort: 'plan' },
  { id: 'e2', from: 'impl',   fromPort: 'diff',     to: 'review',toPort: 'diff' },
  { id: 'e3', from: 'review', fromPort: 'review_feedback', to: 'impl', toPort: 'review_feedback', cond: 'iter < 5 AND verdict ≠ PASS' },
  { id: 'e4', from: 'impl',   fromPort: 'diff',     to: 'tests', toPort: 'diff' },
  { id: 'e5', from: 'review', fromPort: 'verdict',  to: 'merge', toPort: 'verdict', cond: 'verdict == PASS' },
  { id: 'e6', from: 'tests',  fromPort: 'result',   to: 'merge', toPort: 'result' },
  { id: 'e7', from: 'review', fromPort: 'verdict',  to: 'halt',  cond: 'iter ≥ 5', haltMsg: 'Max iterations reached without PASS verdict.' },
];

window.RUNS = RUNS;
window.PIPELINES = PIPELINES;
window.FWR_NODES = FWR_NODES;
window.FWR_EDGES = FWR_EDGES;
