import { useCallback, useEffect } from "react";
import { X } from "lucide-react";

interface Props {
  /** Fully-resolved image URL to display at full size. */
  src: string;
  alt?: string;
  onClose: () => void;
}

/**
 * Full-viewport overlay that shows a single image as large as the viewport
 * allows (capped at 95vw/95vh, scaled down to fit, never blown up past the
 * card it was opened from). Closes on Escape, backdrop click, clicking the
 * image, or the close button.
 *
 * Rendered as a `fixed inset-0` layer above any modal (z-60 > the modal's
 * z-50), so it works both inside MarkdownArtifactModal and standalone from the
 * NodeDetailPanel thumbnails.
 */
export default function ImageLightbox({ src, alt = "", onClose }: Props) {
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Stop the event from also reaching a parent modal's Escape handler.
        e.stopPropagation();
        onClose();
      }
    },
    [onClose],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  return (
    <div
      className="fixed inset-0 z-[60] grid place-items-center p-6"
      style={{ background: "rgba(5,7,10,0.88)", backdropFilter: "blur(6px)" }}
      onClick={(e) => {
        // Don't let the click bubble to a parent modal backdrop.
        e.stopPropagation();
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-label={alt || "Image preview"}
      data-testid="image-lightbox"
    >
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
        className="absolute right-4 top-4 rounded p-1.5 text-fg-3 hover:bg-bg-3 hover:text-fg"
        aria-label="Close image preview"
        data-testid="lightbox-close"
      >
        <X size={18} />
      </button>
      <img
        src={src}
        alt={alt}
        className="max-h-[95vh] max-w-[95vw] cursor-zoom-out rounded object-contain"
        style={{ boxShadow: "0 30px 80px rgba(0,0,0,0.7)" }}
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
        data-testid="lightbox-image"
      />
    </div>
  );
}
