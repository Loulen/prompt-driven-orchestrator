/**
 * The small print the sync popover and the fast-forward card share: a title, a
 * body, and the sentence that states a gap (#802, #803).
 */

import type { BranchGap } from "../model/branchSelect";

export function Title({ icon, children }: { icon: React.ReactNode; children: React.ReactNode }) {
  return (
    <div
      className="flex items-start gap-1.5 font-semibold text-fg"
      style={{ fontSize: "11.5px" }}
      data-testid="sync-popover-title"
    >
      <span className="mt-0.5 shrink-0">{icon}</span>
      <span>{children}</span>
    </div>
  );
}

export function Body({ children }: { children: React.ReactNode }) {
  return (
    <p className="mt-1.5 leading-snug text-fg-3" style={{ fontSize: "10.5px" }}>
      {children}
    </p>
  );
}

/** "main is 3 commits behind origin/main" — plural-aware, both axes named. */
export function GapSentence({
  branchName,
  gap,
  upstream,
}: {
  branchName: string;
  gap: Extract<BranchGap, { kind: "ahead" | "behind" | "diverged" }>;
  upstream: string;
}) {
  const name = <span className="font-mono">{branchName}</span>;
  const up = <span className="font-mono">{upstream}</span>;
  const commits = (n: number) => `${n} commit${n > 1 ? "s" : ""}`;
  if (gap.kind === "behind") {
    return (
      <>
        {name} is <strong>{commits(gap.behind)} behind</strong> {up}
      </>
    );
  }
  if (gap.kind === "ahead") {
    return (
      <>
        {name} is <strong>{commits(gap.ahead)} ahead of</strong> {up}
      </>
    );
  }
  return (
    <>
      {name} has <strong>diverged</strong> from {up} — {gap.ahead} ahead, {gap.behind} behind
    </>
  );
}
