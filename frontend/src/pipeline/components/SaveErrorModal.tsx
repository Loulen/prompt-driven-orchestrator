import { CircleX } from "lucide-react";
import type { SaveErrorData } from "../store/editState";
import { Button, Modal } from "../../shared/ui/kit";
import "./inspect.css";

interface Props {
  open: boolean;
  error: SaveErrorData | null;
  onDismiss: () => void;
  onViewYaml: () => void;
}

/** A Save the daemon refused: its words, the line when it names one, and the way to the YAML. */
export default function SaveErrorModal({ open, error, onDismiss, onViewYaml }: Props) {
  if (!open || !error) return null;

  return (
    <Modal
      size="s"
      title="Save failed"
      icon={<CircleX size={14} strokeWidth={1.5} color="var(--color-st-failed)" aria-hidden />}
      onClose={onDismiss}
      onSubmit={onViewYaml}
      data-testid="save-error-modal"
      actions={
        <>
          <Button onClick={onDismiss} data-testid="save-error-dismiss">
            Close
          </Button>
          <Button tone="primary" onClick={onViewYaml} data-autofocus="" data-testid="save-error-view-yaml">
            View YAML
          </Button>
        </>
      }
    >
      <pre className="ei-quote" data-testid="save-error-message">
        {error.line != null && <span className="ei-dim">line {error.line}: </span>}
        {error.message}
      </pre>
    </Modal>
  );
}
