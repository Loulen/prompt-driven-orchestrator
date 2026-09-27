import TriggerFields from "./TriggerFields";
import AttachmentsField from "./AttachmentsField";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** ── WHAT / WHEN ── the trigger fields, the prompt and the attachments. */
export default function WhatSection({ form }: { form: NewRunForm }) {
  const {
    input, setInput, mode, promptOptional,
  } = form;
  return (
    <div className="flex flex-col gap-3 py-4">
      <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
        {mode === "trigger" ? "When" : "What"}
      </span>

      {/* Schedule (Trigger mode only, #160) */}
      {mode === "trigger" && (
        <TriggerFields form={form} />
      )}

      <div className="flex flex-col gap-1.5">
        <label
          className="font-medium text-fg-2"
          style={{ fontSize: "11.5px" }}
        >
          {mode === "trigger"
            ? "Input template (optional)"
            : `Prompt${promptOptional ? " (optional)" : ""}`}
        </label>
        <textarea
          className="w-full resize-y rounded-md border border-line-strong bg-bg-3 px-2.5 py-2 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
          style={{ fontSize: "12px" }}
          rows={5}
          placeholder="Free-text prompt, a GitHub issue link, or a mix."
          value={input}
          onChange={(e) => setInput(e.target.value)}
          data-testid="input-textarea"
        />
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {mode === "trigger"
            ? "Passed as the Run's input each time the trigger fires. Required unless the pipeline is prompt-not-required."
            : promptOptional
            ? "This pipeline runs without a prompt — anything you enter is passed as additional info."
            : "Free-text prompt, an issue link, or a mix."}
        </span>
      </div>

      {/* #779 Attachments — images as thumbnails, other files as chips, one
          wrapping row. One budget per run, read from Settings. */}
      <AttachmentsField form={form} />
    </div>
  );
}
