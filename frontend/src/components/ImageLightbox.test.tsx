import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";

import ImageLightbox from "./ImageLightbox";

describe("ImageLightbox", () => {
  it("renders the image at the given src", () => {
    render(<ImageLightbox src="/runs/r1/artifact?path=a.png" onClose={() => {}} />);
    const img = screen.getByTestId("lightbox-image");
    expect(img.getAttribute("src")).toBe("/runs/r1/artifact?path=a.png");
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<ImageLightbox src="/x.png" onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the backdrop is clicked", () => {
    const onClose = vi.fn();
    render(<ImageLightbox src="/x.png" onClose={onClose} />);
    fireEvent.click(screen.getByTestId("image-lightbox"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the image itself is clicked", () => {
    const onClose = vi.fn();
    render(<ImageLightbox src="/x.png" onClose={onClose} />);
    fireEvent.click(screen.getByTestId("lightbox-image"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes via the close button", () => {
    const onClose = vi.fn();
    render(<ImageLightbox src="/x.png" onClose={onClose} />);
    fireEvent.click(screen.getByTestId("lightbox-close"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close on other keys", () => {
    const onClose = vi.fn();
    render(<ImageLightbox src="/x.png" onClose={onClose} />);
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    fireEvent.keyDown(window, { key: "a" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("removes its keydown listener on unmount", () => {
    const onClose = vi.fn();
    const { unmount } = render(<ImageLightbox src="/x.png" onClose={onClose} />);
    unmount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });
});
