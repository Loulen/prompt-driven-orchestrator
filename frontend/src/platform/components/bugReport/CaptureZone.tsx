import { useEffect, useMemo, useRef } from "react";
import { ImageIcon, X } from "lucide-react";
import { IconButton } from "../../../shared/ui/kit";
import { imageOf } from "./capture";

/**
 * The screenshot of a report: its thumbnail, removable, and the zone that
 * says how to give one (paste it, or choose a file).
 */
export default function CaptureZone({ capture, onChange }: { capture: Blob | null; onChange: (capture: Blob | null) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const url = useMemo(() => (capture ? URL.createObjectURL(capture) : null), [capture]);
  useEffect(() => () => void (url && URL.revokeObjectURL(url)), [url]);
  return (
    <div className="br-capture">
      {url && (
        <div className="br-capture__thumb" data-testid="bug-report-capture">
          <img src={url} alt="Screenshot" />
          <span className="br-capture__remove">
            <IconButton icon={X} label="Remove the screenshot" size="sm" onClick={() => onChange(null)} />
          </span>
        </div>
      )}
      <div
        className="br-capture__zone"
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          const image = imageOf(e.dataTransfer);
          if (!image) return;
          e.preventDefault();
          onChange(image);
        }}
      >
        <ImageIcon size={14} strokeWidth={1.5} aria-hidden />
        <span>Paste a screenshot (Ctrl+V) or</span>
        <button type="button" className="br-capture__file" onClick={() => input.current?.click()}>
          choose a file
        </button>
        <input
          ref={input}
          type="file"
          accept="image/*"
          hidden
          data-testid="bug-report-capture-file"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) onChange(file);
            e.target.value = "";
          }}
        />
      </div>
    </div>
  );
}
