import { Paperclip, X } from "lucide-react";
import * as newRunForm from "../../model/newRunForm";
import { attachmentBadge, formatAttachmentSize } from "../../model/attachments";
import { Button } from "../../../shared/ui/kit";
import FormRow from "../../../shared/ui/FormRow";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/**
 * #779 the Files row: a chip per attachment (an image's thumbnail, a file's
 * badge), then Attach; a paste anywhere in the dialog or a drop on the row
 * adds. The budget is the row's end, its meter under the chips.
 */
export default function AttachmentsField({ form }: { form: NewRunForm }) {
  const {
    attachments, attachmentError, attachmentHint, attachmentDragOver, attachmentShake,
    fileInputRef, maxAttachmentsMb, maxAttachmentBytes, attachmentsTotal, attachmentsOverLimit,
    overLimitChips, imageCount, fileCount, addAttachments, removeAttachment, handleDrop,
    handleDragOver, handleDragLeave,
  } = form;
  const budget = `${formatAttachmentSize(attachmentsTotal)} / ${maxAttachmentsMb.toFixed(1)} MB`;
  return (
    <FormRow
      label="Files"
      top={attachments.length > 0}
      title={`Passed to the entry node as images and files; paste or drop to add, ${maxAttachmentsMb.toFixed(1)} MB per run`}
      testId="attachments-field"
    >
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
      <div className="ln-col ln-grow">
        <div
          className={attachmentShake ? "ln-drop pdo-attach-shake" : "ln-drop"}
          data-testid="image-drop-zone"
          data-drag-over={attachmentDragOver ? "true" : undefined}
          data-over-limit={attachmentsOverLimit || undefined}
          onDrop={handleDrop}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
        >
          {attachments.map((file, idx) => {
            const over = overLimitChips.has(idx);
            const remove = () => removeAttachment(idx);
            const onKeyDown = (e: React.KeyboardEvent) => {
              if (e.key === "Delete" || e.key === "Backspace") {
                e.preventDefault();
                remove();
              }
            };
            const image = newRunForm.isImageAttachment(file);
            return (
              <span
                key={`${file.name}-${idx}`}
                className="d-chip ln-file"
                data-over-limit={over ? "true" : undefined}
                data-testid={image ? "image-thumbnail" : "file-chip"}
                tabIndex={0}
                onKeyDown={onKeyDown}
                title={over ? `${file.name} · over limit` : file.name}
              >
                {image ? (
                  <img src={URL.createObjectURL(file)} alt={file.name} className="ln-file__thumb" title={file.name} />
                ) : (
                  <span className="ln-file__badge" aria-hidden="true">
                    {attachmentBadge(file.name)}
                  </span>
                )}
                <span className="ln-file__name">{file.name}</span>
                <span className="ln-faint ln-num">{formatAttachmentSize(file.size)}</span>
                <button
                  type="button"
                  className="d-chip__x"
                  onClick={remove}
                  data-testid={image ? "image-remove-button" : "file-remove-button"}
                  aria-label={`Remove ${file.name}`}
                  title="Remove"
                >
                  <X size={11} strokeWidth={1.5} aria-hidden />
                </button>
              </span>
            );
          })}
          <Button
            size="sm"
            icon={Paperclip}
            onClick={() => fileInputRef.current?.click()}
            title={`Attach (paste or drop, ${maxAttachmentsMb.toFixed(1)} MB max)`}
            aria-label={attachments.length > 0 ? "Add more attachments" : "Attach"}
            data-testid={attachments.length > 0 ? "image-add-more-button" : "image-upload-button"}
          >
            Attach
          </Button>
          {attachments.length > 0 && (
            <span
              className={attachmentsOverLimit ? "ln-err ln-rowend" : "ln-faint ln-num ln-rowend"}
              data-testid="attachments-counter"
            >
              {[
                imageCount > 0 ? `${imageCount} image${imageCount > 1 ? "s" : ""}` : null,
                fileCount > 0 ? `${fileCount} file${fileCount > 1 ? "s" : ""}` : null,
              ]
                .filter(Boolean)
                .join(", ")}
              {" · "}
              {budget}
            </span>
          )}
        </div>
        {attachments.length > 0 && (
          <span className="ln-bar" data-testid="attachments-meter" data-over={attachmentsOverLimit || undefined} aria-hidden="true">
            <span style={{ width: `${Math.min(100, (attachmentsTotal / maxAttachmentBytes) * 100)}%` }} />
          </span>
        )}
        {attachmentError ? (
          <span className="ln-err" data-testid="attachments-error">
            {attachmentError}
          </span>
        ) : attachmentsOverLimit ? (
          <span className="ln-err" data-testid="attachments-error">
            Remove {formatAttachmentSize(attachmentsTotal - maxAttachmentBytes)}
          </span>
        ) : attachmentHint ? (
          <span className="ln-faint" data-testid="attachments-hint">
            {attachmentHint}
          </span>
        ) : null}
      </div>
    </FormRow>
  );
}
