import { useCallback, useMemo } from "react";
import * as newRunForm from "../../model/newRunForm";
import {
  DEFAULT_MAX_ATTACHMENTS_MB,
  formatAttachmentSize,
  mergeAttachments,
  overLimitIndices,
} from "../../model/attachments";
import type { NewRunState } from "./useNewRunState";

/**
 * The attachments cluster of the modal (#779, split out of `NewRunModal.tsx`): the per-run
 * budget, the add / remove / paste / drop handlers and the close-with-attachments warning.
 */
export function useAttachments(s: NewRunState, onClose: () => void) {
  const {
    attachments, setAttachments, setAttachmentError, setAttachmentHint, setAttachmentDragOver,
    setAttachmentShake, closeWarning, setCloseWarning, settings,
  } = s;

  // #779: the per-run budget the daemon enforces on `POST /runs`, read from the
  // same setting so the client-side gate and the refusal cannot disagree.
  const maxAttachmentsMb = settings?.max_attachments_mb?.effective ?? DEFAULT_MAX_ATTACHMENTS_MB;
  const maxAttachmentBytes = maxAttachmentsMb * 1024 * 1024;
  const attachmentsTotal = attachments.reduce((sum, f) => sum + f.size, 0);
  const attachmentsOverLimit = attachmentsTotal > maxAttachmentBytes;
  const overLimitChips = useMemo(
    () => overLimitIndices(attachments, maxAttachmentBytes),
    [attachments, maxAttachmentBytes],
  );
  const imageCount = attachments.filter(newRunForm.isImageAttachment).length;
  const fileCount = attachments.length - imageCount;

  const addAttachments = useCallback(
    (files: FileList | File[]) => {
      const incoming = Array.from(files).filter((f) => f.size > 0 || f.type !== "");
      if (incoming.length === 0) return;
      const { next, replaced, refused } = mergeAttachments(attachments, incoming, maxAttachmentBytes);
      setAttachments(next);
      setAttachmentHint(replaced.length > 0 ? `Replaced ${replaced.join(", ")}.` : null);
      if (refused.length > 0) {
        const names = refused.map((f) => `${f.name} (${formatAttachmentSize(f.size)})`).join(", ");
        setAttachmentError(
          `${names} not added: total would exceed ${maxAttachmentsMb.toFixed(1)} MB per run.`,
        );
        setAttachmentShake(true);
        window.setTimeout(() => setAttachmentShake(false), 300);
      } else {
        setAttachmentError(null);
      }
    },
    [attachments, maxAttachmentBytes, maxAttachmentsMb, setAttachmentError, setAttachmentHint, setAttachmentShake, setAttachments],
  );

  const removeAttachment = useCallback((index: number) => {
    setAttachments((prev) => prev.filter((_, i) => i !== index));
    setAttachmentError(null);
    setAttachmentHint(null);
  }, [setAttachmentError, setAttachmentHint, setAttachments]);

  // Modal-wide: pasting a file anywhere in the dialog attaches it; a text paste
  // is left alone (no preventDefault).
  const handlePaste = useCallback(
    (e: React.ClipboardEvent) => {
      const files = e.clipboardData?.files;
      if (files && files.length > 0) {
        e.preventDefault();
        addAttachments(files);
      }
    },
    [addAttachments],
  );

  // Drop target: the zone only, not the whole modal.
  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setAttachmentDragOver(false);
      const files = e.dataTransfer?.files;
      if (files && files.length > 0) {
        addAttachments(files);
      }
    },
    [addAttachments, setAttachmentDragOver],
  );

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setAttachmentDragOver(true);
  }, [setAttachmentDragOver]);

  const handleDragLeave = useCallback(() => setAttachmentDragOver(false), [setAttachmentDragOver]);

  // Close with attachments present: warn once, close on the second ask.
  const requestClose = useCallback(() => {
    if (attachments.length > 0 && !closeWarning) {
      setCloseWarning(true);
      return;
    }
    setCloseWarning(false);
    onClose();
  }, [attachments.length, closeWarning, onClose, setCloseWarning]);

  return {
    maxAttachmentsMb, maxAttachmentBytes, attachmentsTotal, attachmentsOverLimit,
    overLimitChips, imageCount, fileCount, addAttachments, removeAttachment, handlePaste,
    handleDrop, handleDragOver, handleDragLeave, requestClose,
  };
}
