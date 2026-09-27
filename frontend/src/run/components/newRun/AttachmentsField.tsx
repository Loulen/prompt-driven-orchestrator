import { Paperclip, Plus, X } from "lucide-react";
import * as newRunForm from "../../model/newRunForm";
import { attachmentBadge, formatAttachmentSize } from "../../model/attachments";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** #779 Attachments: the drop zone, the thumbnails / chips, the budget meter and messages. */
export default function AttachmentsField({ form }: { form: NewRunForm }) {
  const {
    attachments, attachmentError, attachmentHint, attachmentDragOver, attachmentShake,
    fileInputRef, maxAttachmentsMb, maxAttachmentBytes, attachmentsTotal, attachmentsOverLimit,
    overLimitChips, imageCount, fileCount, addAttachments, removeAttachment, handleDrop,
    handleDragOver, handleDragLeave,
  } = form;
  return (
    <div className="flex flex-col gap-1.5" data-testid="attachments-field">
      <div className="flex items-baseline justify-between gap-2">
        <label
          className="font-medium text-fg-2"
          style={{ fontSize: "11.5px" }}
        >
          Attachments
        </label>
        {attachments.length > 0 && (
          <span
            className={attachmentsOverLimit ? "text-st-failed" : "text-fg-4"}
            style={{ fontSize: "10.5px" }}
            data-testid="attachments-counter"
          >
            {[
              imageCount > 0 ? `${imageCount} image${imageCount > 1 ? "s" : ""}` : null,
              fileCount > 0 ? `${fileCount} file${fileCount > 1 ? "s" : ""}` : null,
            ]
              .filter(Boolean)
              .join(", ")}
            {" · "}
            {formatAttachmentSize(attachmentsTotal)} / {maxAttachmentsMb.toFixed(1)} MB
          </span>
        )}
      </div>
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        data-testid="image-file-input"
        onChange={(e) => {
          if (e.target.files) addAttachments(e.target.files);
          e.target.value = "";
        }}
      />
      <div
        className={`flex min-h-[60px] flex-wrap items-center gap-2 rounded-md border bg-bg-3 px-2.5 py-2 transition-colors ${
          attachmentDragOver
            ? "border-solid border-acc bg-acc/10"
            : attachmentsOverLimit
              ? "border-dashed border-st-failed"
              : "border-dashed border-line-strong hover:border-fg-4"
        } ${attachmentShake ? "pdo-attach-shake border-st-failed" : ""}`}
        data-testid="image-drop-zone"
        data-drag-over={attachmentDragOver ? "true" : undefined}
        onDrop={handleDrop}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
      >
        {attachments.length === 0 && (
          <button
            type="button"
            className="flex w-full items-center justify-center gap-1.5 py-1 text-fg-4 transition-colors hover:text-fg-3"
            style={{ fontSize: "11px" }}
            onClick={() => fileInputRef.current?.click()}
            data-testid="image-upload-button"
          >
            <Paperclip size={14} />
            Paste, drag-drop, or click to add images or files
          </button>
        )}
        {attachments.map((file, idx) => {
          const over = overLimitChips.has(idx);
          const remove = () => removeAttachment(idx);
          const onKeyDown = (e: React.KeyboardEvent) => {
            if (e.key === "Delete" || e.key === "Backspace") {
              e.preventDefault();
              remove();
            }
          };
          if (newRunForm.isImageAttachment(file)) {
            return (
              <div
                key={`${file.name}-${idx}`}
                className={`group relative h-12 w-12 flex-shrink-0 overflow-hidden rounded border focus:outline-none focus:ring-1 focus:ring-acc ${
                  over ? "border-st-failed" : "border-line"
                }`}
                data-testid="image-thumbnail"
                tabIndex={0}
                onKeyDown={onKeyDown}
                title={over ? `${file.name} · over limit` : file.name}
              >
                <img
                  src={URL.createObjectURL(file)}
                  alt={file.name}
                  className="h-full w-full object-cover"
                  title={file.name}
                />
                <button
                  type="button"
                  className="absolute -right-0.5 -top-0.5 grid h-4 w-4 place-items-center rounded-full bg-bg-4 text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
                  onClick={remove}
                  data-testid="image-remove-button"
                  aria-label={`Remove ${file.name}`}
                >
                  <X size={10} />
                </button>
              </div>
            );
          }
          return (
            <div
              key={`${file.name}-${idx}`}
              className={`group relative flex h-12 max-w-[220px] flex-shrink-0 items-center gap-2 rounded border bg-bg-2 px-2 focus:outline-none focus:ring-1 focus:ring-acc ${
                over ? "border-st-failed" : "border-line"
              }`}
              data-testid="file-chip"
              data-over-limit={over ? "true" : undefined}
              tabIndex={0}
              onKeyDown={onKeyDown}
              title={file.name}
            >
              <span
                className="grid h-8 w-8 flex-shrink-0 place-items-center rounded border border-line bg-bg-3 font-mono font-medium text-fg-3"
                style={{ fontSize: "8.5px", letterSpacing: "0.02em" }}
                aria-hidden="true"
              >
                {attachmentBadge(file.name)}
              </span>
              <span className="flex min-w-0 flex-col leading-tight">
                <span className="truncate text-fg" style={{ fontSize: "11.5px" }}>
                  {file.name}
                </span>
                <span
                  className={over ? "text-st-failed" : "text-fg-4"}
                  style={{ fontSize: "10px" }}
                >
                  {formatAttachmentSize(file.size)}
                  {over ? " · over limit" : ""}
                </span>
              </span>
              <button
                type="button"
                className="absolute -right-0.5 -top-0.5 grid h-4 w-4 place-items-center rounded-full bg-bg-4 text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
                onClick={remove}
                data-testid="file-remove-button"
                aria-label={`Remove ${file.name}`}
              >
                <X size={10} />
              </button>
            </div>
          );
        })}
        {attachments.length > 0 && (
          <button
            type="button"
            className="grid h-12 w-12 flex-shrink-0 place-items-center rounded border border-dashed border-line-strong text-fg-4 transition-colors hover:border-fg-3 hover:text-fg-3"
            onClick={() => fileInputRef.current?.click()}
            data-testid="image-add-more-button"
            aria-label="Add more attachments"
          >
            <Plus size={14} />
          </button>
        )}
      </div>
      {attachments.length > 0 && (
        <div
          className="h-[3px] w-full overflow-hidden rounded-full bg-bg-4"
          data-testid="attachments-meter"
          aria-hidden="true"
        >
          <div
            className={`h-full rounded-full transition-[width] ${
              attachmentsOverLimit ? "bg-st-failed" : "bg-acc"
            }`}
            style={{
              width: `${Math.min(100, (attachmentsTotal / maxAttachmentBytes) * 100)}%`,
            }}
          />
        </div>
      )}
      {attachmentError ? (
        <span
          className="text-st-failed"
          style={{ fontSize: "10.5px" }}
          data-testid="attachments-error"
        >
          {attachmentError}
        </span>
      ) : attachmentsOverLimit ? (
        <span
          className="text-st-failed"
          style={{ fontSize: "10.5px" }}
          data-testid="attachments-error"
        >
          Total exceeds {maxAttachmentsMb.toFixed(1)} MB per run. Remove{" "}
          {formatAttachmentSize(attachmentsTotal - maxAttachmentBytes)} to continue.
        </span>
      ) : (
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {attachmentHint ? `${attachmentHint} ` : ""}
          {attachments.length > 0 ? (
            <>
              Passed to the entry node as{" "}
              <span className="font-mono">images</span> and{" "}
              <span className="font-mono">files</span>.
            </>
          ) : (
            `Optional — passed to the entry node. Up to ${maxAttachmentsMb.toFixed(1)} MB per run (configurable in Settings).`
          )}
        </span>
      )}
    </div>
  );
}
