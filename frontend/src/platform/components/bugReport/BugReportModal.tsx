import { useMemo, useState } from "react";
import { Bug, CircleDot, ExternalLink } from "lucide-react";
import { Button, ConfirmDialog, Modal, Select, Tabs, TextArea } from "../../../shared/ui/kit";
import type { PipelineListEntry } from "../../../shared/generated/PipelineListEntry";
import {
  DESCRIPTION_MISSING,
  GITHUB_DESTINATION,
  PIPELINE_UNREADABLE,
  RUN_UNREADABLE,
  blockers,
  parseReport,
  reportFields,
  reportMarkdown,
  type BugReportDestination,
} from "../../model/bugReport";
import type { BugReportPrefill } from "../../store/bugReport";
import { usePastedImage } from "./capture";
import ReportTab, { type ReportForm } from "./ReportTab";
import { sendToGithub } from "./sendReport";
import { useChoices, useInstanceFacts, usePipelineFacts, useRunFacts } from "./useBugReportData";
import "./bugReport.css";

type Tab = "report" | "preview";

export const READING_DETAILS = "Reading the details to join…";

/** The Pipeline a name designates: by its id, else by its name. */
function pipelineNamed(pipelines: PipelineListEntry[], name: string | null | undefined): string | null {
  if (!name) return null;
  return (pipelines.find((p) => p.id === name) ?? pipelines.find((p) => p.name === name))?.id ?? null;
}

/**
 * The Bug report window (CONTEXT.md « Rapport de bug »). Report: a
 * description, a screenshot (pasted anywhere in the window, or chosen), the
 * Run and the Pipeline to join, checked by default and prefilled with the
 * selection; the prompts, greyed toward a public tracker. Preview: the
 * markdown that leaves, the facts of the instance included, editable; once
 * edited, its tab says so and a change of the form asks before overwriting it.
 * A checked box without its choice, or no description, holds the primary
 * button, its tooltip saying why. Ctrl+Enter sends from either tab.
 */
export default function BugReportModal({
  prefill,
  onClose,
  destinations = [GITHUB_DESTINATION],
}: {
  prefill: BugReportPrefill;
  onClose: () => void;
  /** Where the report may go, the core's first. */
  destinations?: BugReportDestination[];
}) {
  const facts = useInstanceFacts();
  const { runs, pipelines, pipelinesRead } = useChoices();
  const [now] = useState(() => Date.now());
  const [tab, setTab] = useState<Tab>("report");
  const [destinationId, setDestinationId] = useState(destinations[0].id);
  const destination = destinations.find((d) => d.id === destinationId) ?? destinations[0];
  const [capture, setCapture] = useState<Blob | null>(null);
  usePastedImage(setCapture);

  const [form, setForm] = useState<ReportForm>({
    description: "",
    includeRun: true,
    runId: prefill.runId,
    includePipeline: true,
    pipelineId: null,
    includePrompts: false,
  });
  // The Pipeline follows the prefill (or the prefilled Run's) until the user touches it.
  const [pipelineTouched, setPipelineTouched] = useState(false);
  const run = useRunFacts(form.includeRun ? form.runId : null);
  const prefilledRun = runs.find((r) => r.run_id === prefill.runId);
  const pipelineId = pipelineTouched
    ? form.pipelineId
    : pipelineNamed(
        pipelines,
        prefill.pipeline ?? prefilledRun?.pipeline_name ?? (run && form.runId === prefill.runId ? run.pipeline_name : null),
      );
  const current: ReportForm = { ...form, pipelineId };
  const pipeline = usePipelineFacts(current.includePipeline ? pipelineId : null);

  const fields = useMemo(
    () =>
      reportFields({
        description: form.description,
        facts,
        run: run ?? null,
        pipeline: pipeline ?? null,
        includePrompts: form.includePrompts,
        destination,
        now,
      }),
    [form.description, form.includePrompts, facts, run, pipeline, destination, now],
  );
  const generated = useMemo(() => reportMarkdown(fields), [fields]);
  const [edited, setEdited] = useState<string | null>(null);
  const [pending, setPending] = useState<(() => void) | null>(null);

  /** A change of the form, asked first when it would overwrite an edited preview. */
  const guard = (apply: () => void) => {
    if (edited === null) apply();
    else setPending(() => apply);
  };
  const change = (patch: Partial<ReportForm>) =>
    guard(() => {
      if ("pipelineId" in patch || "includePipeline" in patch) setPipelineTouched(true);
      setForm((f) => ({ ...f, pipelineId, ...patch }));
    });

  const blocked = blockers(current);
  // The prefilled Pipeline is not known before the list is read: nothing is missing yet.
  const resolving = !pipelineTouched && !pipelinesRead;
  const missing = {
    run: blocked.run ?? (current.includeRun && current.runId && run === null ? RUN_UNREADABLE : undefined),
    pipeline: resolving
      ? undefined
      : (blocked.pipeline ?? (current.includePipeline && pipelineId && pipeline === null ? PIPELINE_UNREADABLE : undefined)),
  };
  const reading =
    resolving ||
    (current.includeRun && !!current.runId && run === undefined) ||
    (current.includePipeline && !!pipelineId && pipeline === undefined);
  // An edited Preview leaves as it is: its description is the one that must not be empty.
  const noDescription = edited !== null ? !parseReport(edited).description?.trim() : !!blocked.description;
  const refusal =
    missing.run ?? missing.pipeline ?? (noDescription ? DESCRIPTION_MISSING : undefined) ?? (reading ? READING_DETAILS : undefined);

  const send = () => {
    if (refusal) return;
    sendToGithub(edited !== null ? parseReport(edited) : fields, capture);
    onClose();
  };

  const destinationOptions = destinations.map((d) => ({
    value: d.id,
    label: d.label,
    group: d.group,
    hint: d.hint,
    icon: CircleDot,
  }));

  return (
    <Modal
      size="l"
      title="Report a bug"
      icon={<Bug size={14} strokeWidth={1.5} aria-hidden />}
      onClose={onClose}
      onSubmit={send}
      data-testid="bug-report"
      footerNote={
        <span className="br-dest" data-testid="bug-report-destination">
          Send to
          <Select
            label="Send to"
            size="sm"
            value={destination.id}
            options={destinationOptions}
            onChange={(id) => guard(() => setDestinationId(id))}
          />
        </span>
      }
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button tone="primary" icon={ExternalLink} disabledReason={refusal} onClick={send} data-testid="bug-report-send">
            {destination.sendLabel}
          </Button>
        </>
      }
    >
      <div className="br-body">
        <div className="br-tabs">
          <Tabs<Tab>
            label="Bug report"
            value={tab}
            onChange={setTab}
            items={[
              { value: "report", label: "Report" },
              { value: "preview", label: "Preview", mark: edited !== null ? "edited" : undefined },
            ]}
          />
        </div>
        {tab === "report" ? (
          <ReportTab
            form={current}
            change={change}
            runs={runs}
            pipelines={pipelines}
            capture={capture}
            onCapture={setCapture}
            missing={missing}
            destination={destination}
          />
        ) : (
          <>
            <TextArea
              mono
              className="br-preview"
              aria-label="Preview"
              value={edited ?? generated}
              onChange={(e) => setEdited(e.target.value)}
              data-testid="bug-report-preview"
            />
            <div className="br-preview__foot">
              <span>Edits here are what gets sent. Changing the form afterwards asks before overwriting them.</span>
              <Button tone="ghost" size="sm" disabled={edited === null} onClick={() => setEdited(null)} data-testid="bug-report-reset">
                Reset preview
              </Button>
            </div>
          </>
        )}
      </div>
      {pending && (
        <ConfirmDialog
          title="Overwrite the edited preview?"
          consequence="Your edits to the preview are lost"
          actionLabel="Overwrite"
          cancelLabel="Keep my edits"
          onConfirm={() => {
            pending();
            setEdited(null);
            setPending(null);
          }}
          onClose={() => setPending(null)}
        />
      )}
    </Modal>
  );
}
