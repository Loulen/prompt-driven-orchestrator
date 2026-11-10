import { ArrowLeft } from "lucide-react";

/**
 * The bar over a child run opened from an Orchestration tab (#723): the parent
 * run and its parent node. The whole bar leads back to them.
 */
export default function ChildRunBar({
  parentRunName,
  nodeName,
  onBack,
}: {
  parentRunName: string;
  nodeName: string;
  onBack: () => void;
}) {
  return (
    <button
      type="button"
      className="cv-child"
      data-testid="orchestrator-back-bar"
      title={`Back to ${parentRunName} · ${nodeName}`}
      onClick={onBack}
    >
      <span className="cv-child__back" data-testid="orchestrator-back">
        <ArrowLeft size={14} strokeWidth={1.5} aria-hidden />
      </span>
      <span className="cv-child__parent">{parentRunName}</span>
      <span className="cv-child__node">{nodeName}</span>
    </button>
  );
}
