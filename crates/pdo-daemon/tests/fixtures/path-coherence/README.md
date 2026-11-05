# Path-coherence fixture corpus (ADR-0081)

One small pipeline per case (`<case>.yaml`) and, in `expected.json`, the defects
the path-coherence check must report for it — kind (`no_path` / `dead_end` /
`orphan`), culprit node id, exact message, **in order** (no path, then every dead
end, then every orphan, each in node order).

Shared by the daemon (`crates/pdo-daemon/src/path_coherence.rs`, the authority)
and the editor's TypeScript mirror (#1022): both must agree on every case. Add a
case here, never in one side only.
