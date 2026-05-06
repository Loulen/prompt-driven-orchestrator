declare module "ansi-to-html" {
  interface Options {
    fg?: string;
    bg?: string;
    newline?: boolean;
    escapeXML?: boolean;
    stream?: boolean;
    colors?: string[] | Record<number, string>;
  }
  class Convert {
    constructor(options?: Options);
    toHtml(input: string): string;
  }
  export default Convert;
}
