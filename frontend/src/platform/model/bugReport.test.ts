import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  GITHUB_DESTINATION,
  PIPELINE_MISSING,
  RUN_MISSING,
  blockers,
  parseReport,
  reportFields,
  reportMarkdown,
  sendRefusal,
  type BugReportDestination,
  type BugReportInput,
} from "./bugReport";
import { pipelineDetails, runDetails, type PipelineFacts, type RunFacts } from "./bugReportDetails";
import { BUG_REPORT_TEMPLATE, CORE_ISSUES_NEW_URL, TEMPLATE_FIELDS, dropRows, githubIssueUrl, issueTitle } from "./bugReportGithub";

const NOW = Date.parse("2026-11-20T12:00:00Z");

function run(overrides: Partial<RunFacts> = {}): RunFacts {
  return {
    run_id: "20261120-100000-abcd",
    name: "nightly",
    pipeline_name: "ship",
    status: "failed",
    started_at: "2026-11-20T10:00:00Z",
    completed_at: "2026-11-20T11:30:00Z",
    failure_reason: "node build failed",
    nodes: {
      build: {
        node_id: "build",
        status: "failed",
        iter: 1,
        started_at: "2026-11-20T10:00:00Z",
        completed_at: "2026-11-20T10:04:12Z",
        failure_reason: "exit 1 | see log\nsecond line",
        iterations: [],
        frontmatter_retries: 0,
      },
      check: {
        node_id: "check",
        status: "pending",
        iter: 0,
        started_at: null,
        completed_at: null,
        failure_reason: null,
        iterations: [],
        frontmatter_retries: 0,
      },
    },
    node_defs: [
      { id: "start", node_type: "start", view_x: null, view_y: null, inputs: [], outputs: [] },
      { id: "build", name: "Build", node_type: "agent", view_x: null, view_y: null, inputs: [], outputs: [] },
      { id: "check", node_type: "script", view_x: null, view_y: null, inputs: [], outputs: [] },
      { id: "end", node_type: "end", view_x: null, view_y: null, inputs: [], outputs: [] },
    ],
    ...overrides,
  };
}

const PIPELINE: PipelineFacts = {
  name: "ship",
  nodes: [
    { id: "build", name: "Build", type: "agent", agent_choice: { mode: "custom", harness: "claude", model: "opus" }, isolated_worktree: true },
    { id: "check", name: null, type: "script" },
  ],
  edges: [
    { source: { node: "build", port: "out" }, target: { node: "check", port: "in" }, when: { verdict: "PASS" } },
    { source: { node: "check", port: "out" }, target: { node: "build", port: "in" }, else: true },
  ],
  prompts: { build: "SECRET PROMPT: build the thing" },
};

const FACTS = { version: "PDO 2.81.0", os: "Linux 6.8.0-107-generic", harnesses: [{ name: "claude", version: "2.1.322" }, { name: "pi", version: null }] };

function input(overrides: Partial<BugReportInput> = {}): BugReportInput {
  return {
    description: "The node stays running",
    facts: FACTS,
    run: null,
    pipeline: null,
    includePrompts: false,
    destination: GITHUB_DESTINATION,
    now: NOW,
    ...overrides,
  };
}

const INTERNAL: BugReportDestination = { ...GITHUB_DESTINATION, id: "internal", acceptsPrompts: true };

describe("reportFields", () => {
  it("always carries the version, the OS and the harnesses with their versions", () => {
    const fields = reportFields(input());
    expect(fields).toEqual({
      description: "The node stays running",
      version: "PDO 2.81.0",
      os: "Linux 6.8.0-107-generic",
      harnesses: "claude 2.1.322 · pi",
    });
  });

  it("says unknown while the facts are not read", () => {
    expect(reportFields(input({ facts: null }))).toMatchObject({ version: "unknown", os: "unknown", harnesses: "unknown" });
  });

  it("joins the run and the pipeline details only when given", () => {
    const fields = reportFields(input({ run: run(), pipeline: PIPELINE }));
    expect(fields.run).toContain("Run 20261120-100000-abcd · nightly · failed · 1h 30m");
    expect(fields.pipeline).toContain("Pipeline ship · 2 nodes · 2 edges");
  });

  it("never joins the prompts toward GitHub, even asked for", () => {
    const fields = reportFields(input({ pipeline: PIPELINE, includePrompts: true }));
    expect(fields.prompts).toBeUndefined();
    expect(JSON.stringify(fields)).not.toContain("SECRET PROMPT");
  });

  it("joins the prompts toward a destination that accepts them, when asked", () => {
    expect(reportFields(input({ pipeline: PIPELINE, includePrompts: true, destination: INTERNAL })).prompts).toBe(
      "#### Build\nSECRET PROMPT: build the thing",
    );
    expect(reportFields(input({ pipeline: PIPELINE, includePrompts: false, destination: INTERNAL })).prompts).toBeUndefined();
  });
});

describe("runDetails", () => {
  it("gives the run's status, each node's status, the failure reasons and the durations", () => {
    expect(runDetails(run(), NOW)).toBe(
      [
        "Run 20261120-100000-abcd · nightly · failed · 1h 30m",
        "Failure: node build failed",
        "",
        "| Node | Type | Status | Duration | Failure |",
        "| --- | --- | --- | --- | --- |",
        "| Build | agent | failed | 4m 12s | exit 1 \\| see log second line |",
        "| check | script | pending |  |  |",
      ].join("\n"),
    );
  });

  it("counts a live run up to now", () => {
    expect(runDetails(run({ status: "running", completed_at: null, failure_reason: undefined }), NOW)).toMatch(
      /^Run 20261120-100000-abcd · nightly · running · 2h 0m$/m,
    );
  });
});

describe("pipelineDetails", () => {
  it("gives the structure: nodes, types, harness, isolation and edges, without a prompt", () => {
    const text = pipelineDetails(PIPELINE);
    expect(text).toBe(
      [
        "Pipeline ship · 2 nodes · 2 edges",
        "",
        "| Node | Type | Harness | Isolation |",
        "| --- | --- | --- | --- |",
        "| Build | agent | claude opus | isolated |",
        "| check | script |  |  |",
        "",
        "Edges:",
        "- Build.out -> check.in (conditional)",
        "- check.out -> Build.in (else)",
      ].join("\n"),
    );
    expect(text).not.toContain("SECRET");
  });
});

describe("the preview", () => {
  it("is one heading per section present, read back into the same fields", () => {
    const fields = reportFields(input({ run: run(), pipeline: PIPELINE }));
    const text = reportMarkdown(fields);
    expect(text.startsWith("### Description\nThe node stays running\n\n### Version\nPDO 2.81.0")).toBe(true);
    expect(parseReport(text)).toEqual(fields);
  });

  it("keeps what an edit wrote: a removed section is gone, text before any heading is description", () => {
    const fields = parseReport("Some context\n### Version\nPDO 1\n### Unknown heading\nstays in Version");
    expect(fields).toEqual({ description: "Some context", version: "PDO 1\n### Unknown heading\nstays in Version" });
  });
});

describe("blockers", () => {
  const form = { description: "x", includeRun: true, runId: "r", includePipeline: true, pipelineId: "p" };

  it("lets a complete form go", () => {
    expect(blockers(form)).toEqual({});
    expect(sendRefusal(form)).toBeUndefined();
  });

  it("blocks a checked box without its choice, and unchecking it unblocks", () => {
    expect(blockers({ ...form, runId: null })).toEqual({ run: RUN_MISSING });
    expect(sendRefusal({ ...form, runId: null })).toBe(RUN_MISSING);
    expect(sendRefusal({ ...form, runId: null, includeRun: false })).toBeUndefined();
    expect(blockers({ ...form, pipelineId: null })).toEqual({ pipeline: PIPELINE_MISSING });
    expect(sendRefusal({ ...form, pipelineId: null, includePipeline: false })).toBeUndefined();
  });

  it("asks for a description", () => {
    expect(sendRefusal({ ...form, description: "  " })).toBe("Describe the bug first");
  });
});

describe("githubIssueUrl", () => {
  it("opens a new issue of the public repository on the Bug report template, its fields prefilled", () => {
    const fields = reportFields(input({ run: run(), pipeline: PIPELINE, includePrompts: true }));
    const url = new URL(githubIssueUrl(fields));
    expect(`${url.origin}${url.pathname}`).toBe(CORE_ISSUES_NEW_URL);
    expect(url.searchParams.get("template")).toBe(BUG_REPORT_TEMPLATE);
    expect(url.searchParams.get("title")).toBe("The node stays running");
    expect(url.searchParams.get("description")).toBe("The node stays running");
    expect(url.searchParams.get("version")).toBe("PDO 2.81.0");
    expect(url.searchParams.get("os")).toBe("Linux 6.8.0-107-generic");
    expect(url.searchParams.get("harnesses")).toBe("claude 2.1.322 · pi");
    expect(url.searchParams.get("run")).toBe(fields.run);
    expect(url.searchParams.get("pipeline")).toBe(fields.pipeline);
    expect(url.searchParams.has("prompts")).toBe(false);
  });

  it("drops the prompts of an edited preview too", () => {
    const url = githubIssueUrl({ description: "d", prompts: "SECRET PROMPT" });
    expect(url).not.toContain("SECRET");
  });

  it("drops node rows of a run too long for a URL, and says how many", () => {
    const defs = Array.from({ length: 200 }, (_, i) => ({
      id: `n${i}`,
      node_type: "agent" as const,
      view_x: null,
      view_y: null,
      inputs: [],
      outputs: [],
    }));
    const fields = reportFields(input({ run: run({ node_defs: defs, nodes: {} }) }));
    const url = githubIssueUrl(fields, 3000);
    expect(url.length).toBeLessThanOrEqual(3000);
    const sent = new URL(url).searchParams.get("run")!;
    const omitted = Number(/\(truncated, (\d+) nodes omitted\)/.exec(sent)?.[1]);
    expect(omitted).toBeGreaterThan(0);
    expect(sent.split("\n").filter((l) => l.startsWith("| n")).length).toBe(200 - omitted);
  });

  it("stays under the limit when even the description is too long", () => {
    const url = githubIssueUrl({ description: "é".repeat(5000) }, 2000);
    expect(url.length).toBeLessThanOrEqual(2000);
  });
});

describe("helpers", () => {
  it("dropRows keeps the header and says how many rows went", () => {
    const text = "head\n\n| A |\n| --- |\n| 1 |\n| 2 |\n| 3 |";
    expect(dropRows(text, 2)).toBe("head\n\n| A |\n| --- |\n| 1 |\n(truncated, 2 nodes omitted)");
    expect(dropRows("no table", 2)).toBe("no table");
  });

  it("issueTitle takes the first line, shortened", () => {
    expect(issueTitle("\n  first line \nsecond")).toBe("first line");
    expect(issueTitle("x".repeat(100))).toHaveLength(80);
  });
});

describe("the repository's Bug report template", () => {
  const template = readFileSync(join(import.meta.dirname, "../../../../.github/ISSUE_TEMPLATE", BUG_REPORT_TEMPLATE), "utf8");

  it("has a field for each section the URL prefills, and none for the prompts", () => {
    const ids = [...template.matchAll(/^\s+id:\s*(\w+)\s*$/gm)].map((m) => m[1]);
    expect(ids).toEqual(expect.arrayContaining([...TEMPLATE_FIELDS]));
    expect(ids).not.toContain("prompts");
  });

  it("is labelled for triage", () => {
    expect(template).toMatch(/^labels: \["needs-triage"\]$/m);
  });
});
