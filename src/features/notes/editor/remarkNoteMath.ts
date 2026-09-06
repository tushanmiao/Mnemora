import type { Root, Parent } from "mdast";
import type { VFile } from "vfile";
import { isCurrencyMath } from "./noteSyntax";

export function remarkNoteMath() {
  return (root: Root, file: VFile) => {
    const source = String(file);
    const walk = (parent: Parent) => {
      parent.children = parent.children.map((node) => {
        if (node.type === "inlineMath" && node.position && isCurrencyMath(node.value, source.slice(node.position.end.offset!, node.position.end.offset! + 1))) {
          return { type: "text" as const, value: source.slice(node.position.start.offset!, node.position.end.offset!), position: node.position };
        }
        if ("children" in node) walk(node as Parent);
        return node;
      });
    };
    walk(root);
  };
}
