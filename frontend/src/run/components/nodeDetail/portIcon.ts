import { FileCode, FileImage, FileText, Files, Images, type LucideIcon } from "lucide-react";

/** The icon of a port by its type, shared by an Output card and a Live artifact card. */
export const PORT_ICON: Record<string, LucideIcon> = {
  markdown: FileText,
  image: FileImage,
  image_list: Images,
  html: FileCode,
  files: Files,
};
