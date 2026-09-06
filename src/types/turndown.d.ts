declare module "turndown" {
  export type TurndownOptions = {
    headingStyle?: "setext" | "atx";
    codeBlockStyle?: "indented" | "fenced";
    bulletListMarker?: "*" | "-" | "+";
  };
  export type Rule = {
    filter: string | string[] | ((node: Node) => boolean);
    replacement: (content: string, node: Node) => string;
  };
  export default class TurndownService {
    constructor(options?: TurndownOptions);
    use(plugin: (service: TurndownService) => void): this;
    addRule(key: string, rule: Rule): this;
    turndown(source: string | Node): string;
  }
}

declare module "turndown-plugin-gfm" {
  import type TurndownService from "turndown";
  export function gfm(service: TurndownService): void;
}
