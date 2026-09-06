import type { Root, PhrasingContent, Parent } from "mdast";

/** Only text nodes are expanded; code, URLs, HTML and math stay untouched. */
export function remarkNoteHighlight() {
  return (root: Root) => {
    const walk = (parent: Parent) => {
      const next = [];
      for (const node of parent.children) {
        if (node.type !== "text") {
          if ("children" in node) walk(node as Parent);
          next.push(node); continue;
        }
        let cursor = 0;
        for (const match of node.value.matchAll(/==([^=\n]+)==/g)) {
          const start = match.index!;
          if (start > cursor) next.push({ type: "text" as const, value: node.value.slice(cursor, start) });
          next.push({ type: "emphasis" as const, data: { hName: "mark" }, children: [{ type: "text", value: match[1] }] as PhrasingContent[] });
          cursor = start + match[0].length;
        }
        if (cursor < node.value.length) next.push({ ...node, value: node.value.slice(cursor) });
      }
      parent.children = next;
    };
    walk(root);
  };
}
