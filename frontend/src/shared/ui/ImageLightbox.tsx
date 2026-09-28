import { useCallback, useEffect, useState } from "react";
import { X, ChevronLeft, ChevronRight } from "lucide-react";

interface Props {
  /** Ordered, fully-resolved image URLs the viewer can page through. */
  images: string[];
  /** Index into `images` of the image that was clicked (seeds the cursor). */
  index: number;
  alt?: string;
  onClose: () => void;
}

/**
 * Full-viewport overlay that shows an image as large as the viewport allows
 * (capped at 95vw/95vh, scaled down to fit, never blown up past the card it
 * was opened from). Closes on Escape, backdrop click, clicking the image, or
 * the close button.
 *
 * List-aware (#312): given the whole ordered `images` list plus the clicked
 * `index`, the left/right arrow keys (and the ‹ › chevrons) move between them.
 * Navigation clamps at both ends — arrowing past the last/first image is a
 * no-op, matching every other sequence-nav surface in the app (no wrap). The
 * chevrons + "N of M" counter only render when there is more than one image.
 *
 * Rendered as a `fixed inset-0` layer above any modal (z-60 > the modal's
 * z-50), so it works both inside MarkdownArtifactModal and standalone from the
 * NodeDetailPanel thumbnails.
 */
export default function ImageLightbox({ images, index, alt = "", onClose }: Props) {
  const [current, setCurrent] = useState(index);
  // Re-seed if a caller reopens at a different index WITHOUT unmounting
  // ("adjust state during render" — no effect, no flash). Today every caller
  // conditionally renders the lightbox so it remounts on each open, but this
  // keeps us correct if a caller ever swaps images while mounted.
  const [seed, setSeed] = useState(index);
  if (index !== seed) {
    setSeed(index);
    setCurrent(index);
  }

  const hasPrev = current > 0;
  const hasNext = current < images.length - 1;
  const goPrev = useCallback(() => setCurrent((c) => Math.max(0, c - 1)), []);
  const goNext = useCallback(
    () => setCurrent((c) => Math.min(images.length - 1, c + 1)),
    [images.length],
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Stop the event from also reaching a parent modal's Escape handler.
        e.stopPropagation();
        onClose();
      } else if (e.key === "ArrowLeft") {
        // stopPropagation is hygiene; preventDefault stops the background page
        // from scrolling on the arrow key.
        e.stopPropagation();
        e.preventDefault();
        goPrev();
      } else if (e.key === "ArrowRight") {
        e.stopPropagation();
        e.preventDefault();
        goNext();
      }
    },
    [onClose, goPrev, goNext],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  const src = images[current] ?? images[0];

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
      {images.length > 1 && (
        <>
          <button
            type="button"
            onClick={(e) => {
              // Mandatory: the overlay and the image both close on click, so an
              // un-stopped chevron click would close the viewer.
              e.stopPropagation();
              goPrev();
            }}
            disabled={!hasPrev}
            className="absolute left-4 top-1/2 -translate-y-1/2 rounded p-1.5 text-fg-3 hover:bg-bg-3 hover:text-fg disabled:opacity-30"
            aria-label="Previous image"
            data-testid="lightbox-prev"
          >
            <ChevronLeft size={28} />
          </button>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              goNext();
            }}
            disabled={!hasNext}
            className="absolute right-4 top-1/2 -translate-y-1/2 rounded p-1.5 text-fg-3 hover:bg-bg-3 hover:text-fg disabled:opacity-30"
            aria-label="Next image"
            data-testid="lightbox-next"
          >
            <ChevronRight size={28} />
          </button>
          <span
            className="absolute bottom-4 left-1/2 -translate-x-1/2 font-mono text-fg-3"
            style={{ fontSize: "11px" }}
            data-testid="lightbox-counter"
          >
            {current + 1} of {images.length}
          </span>
        </>
      )}
    </div>
  );
}
