import type { PortType } from "../../shared/generated/PortType";

/** The types an output or a live artifact can declare, as the type picker lists them. */
export const PORT_TYPE_OPTIONS: { value: PortType; label: string }[] = [
  { value: "markdown", label: "Markdown" },
  { value: "image", label: "Image" },
  { value: "image_list", label: "Image List" },
  { value: "html", label: "HTML" },
];
