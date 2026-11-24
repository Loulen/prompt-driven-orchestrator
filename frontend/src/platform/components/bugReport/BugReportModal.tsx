import { useMemo, useState } from "react";
import { Bug, ExternalLink, LoaderCircle, Send } from "lucide-react";
import { Banner, Button, ConfirmDialog, Modal, Select, Tabs, TextArea, type Icon } from "../../../shared/ui/kit";
import type { PipelineListEntry } from "../../../shared/generated/PipelineListEntry";
import {
  DESCRIPTION_MISSING,
  PIPELINE_UNREADABLE,
  RUN_UNREADABLE,
  blockers,
  parseReport,
  reportFields,
  reportMarkdown,
} from "../../model/bugReport";
import { initialDestination, opensInBrowser } from "../../model/bugReportDestinations";
import { rememberBugReportDestination } from "../../api/bugReport";
import type { BugReportPrefill } from "../../store/bugReport";
import { usePastedImage } from "./capture";
import ReportTab, { type ReportForm } from "./ReportTab";
import { useDestinationIcons } from "./useDestinationIcons";
import { sendToExtension, sendToGithub, toastSent } from "./sendReport";
import { useChoices, useDestinations, useInstanceFacts, usePipelineFacts, useRunFacts } from "./useBugReportData";
import "./bugReport.css";

type Tab = "report" | "preview";

export const READING_DETAILS = "Reading the details to join…";
export const NOT_SENT = "The bug report was not sent";

/** The primary button's spinner while the report leaves. */
const Spinning: Icon = (props) => <LoaderCircle {...props} className="d-spin" />;

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
 *
 * « Send to » (CONTEXT.md « Destination de bug report »): the core's issues
 * first, then the extensions' destinations under their names, starting on the
 * caller's last choice. Toward an extension, the window stays locked while the
 * report leaves, then closes on a toast linking to what the destination
 * created; a failure stays in the window, in a Banner.
 */
export default function BugReportModal({ prefill, onClose }: { prefill: BugReportPrefill; onClose: () => void }) {
  const facts = useInstanceFacts();
  const { runs, pipelines, pipelinesRead } = useChoices();
  const [now] = useState(() => Date.now());
  const [tab, setTab] = useState<Tab>("report");
  const { destinations, last } = useDestinations();
  // The last choice until the user picks one.
  const [picked, setPicked] = useState<string | null>(null);
  const destinationId = picked ?? initialDestination(destinations, last);
  const destination = destinations.find((d) => d.id === destinationId) ?? destinations[0];
  const icons = useDestinationIcons(destinations);
  const [sending, setSending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
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
    if (refusal || sending) return;
    const outgoing = edited !== null ? parseReport(edited) : fields;
    if (opensInBrowser(destination)) {
      sendToGithub(outgoing, capture);
      void rememberBugReportDestination(destination.id).catch(() => {});
      onClose();
      return;
    }
    setSending(true);
    setFailure(null);
    sendToExtension(destination, edited ?? generated, outgoing, capture).then(
      (sent) => {
        onClose();
        toastSent(sent);
      },
      (e: unknown) => {
        setSending(false);
        setFailure(e instanceof Error ? e.message : String(e));
      },
    );
  };

  const destinationOptions = destinations.map((d) => ({
    value: d.id,
    label: d.label,
    group: d.group,
    hint: d.hint,
    icon: icons.get(d.id),
  }));

  return (
    <Modal
      size="l"
      title="Report a bug"
      icon={<Bug size={14} strokeWidth={1.5} aria-hidden />}
      onClose={onClose}
      // Locked while the report leaves: no Cancel, no close, no Escape.
      dismissable={!sending}
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
            onChange={(id) =>
              guard(() => {
                setPicked(id);
                setFailure(null);
              })
            }
            disabled={sending}
          />
        </span>
      }
      actions={
        <>
          <Button onClick={onClose} disabled={sending}>
            Cancel
          </Button>
          <Button
            tone="primary"
            icon={sending ? Spinning : opensInBrowser(destination) ? ExternalLink : Send}
            disabledReason={sending ? undefined : refusal}
            disabled={sending}
            data-pending={sending ? "" : undefined}
            onClick={send}
            data-testid="bug-report-send"
          >
            {sending ? "Sending…" : destination.sendLabel}
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
              onChange={(e) => {
                // An edited preview keeps its destination: the last choice read late no longer moves it.
                if (picked === null) setPicked(destinationId);
                setEdited(e.target.value);
              }}
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
        {failure && <Banner tone="failed" title={NOT_SENT} text={failure} />}
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
