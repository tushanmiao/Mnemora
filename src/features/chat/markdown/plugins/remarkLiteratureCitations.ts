import type { LiteratureReference } from "../../../../types/chat";

type MarkdownNode = {
  type: string;
  value?: string;
  url?: string;
  children?: MarkdownNode[];
};

function citationLabels(reference: LiteratureReference) {
  const page = reference.pageIndex + 1;
  return [
    `【${reference.title}，第 ${page} 页】`,
    `【${reference.title}, 第 ${page} 页】`,
    `【${reference.title}，第${page}页】`,
  ];
}

function replaceCitations(node: MarkdownNode, references: readonly LiteratureReference[]) {
  if (!node.children || node.type === "code" || node.type === "inlineCode" || node.type === "link") return;
  const nextChildren: MarkdownNode[] = [];
  for (const child of node.children) {
    if (child.type !== "text" || !child.value) {
      replaceCitations(child, references);
      nextChildren.push(child);
      continue;
    }
    let remaining = child.value;
    while (remaining) {
      let best: { index: number; label: string; reference: LiteratureReference } | null = null;
      for (const reference of references) {
        for (const label of citationLabels(reference)) {
          const index = remaining.indexOf(label);
          if (index >= 0 && (!best || index < best.index)) best = { index, label, reference };
        }
      }
      if (!best) {
        nextChildren.push({ type: "text", value: remaining });
        break;
      }
      if (best.index > 0) nextChildren.push({ type: "text", value: remaining.slice(0, best.index) });
      nextChildren.push({
        type: "link",
        url: `mnemora-citation:${encodeURIComponent(best.reference.id)}`,
        children: [{ type: "text", value: best.label }],
      });
      remaining = remaining.slice(best.index + best.label.length);
    }
  }
  node.children = nextChildren;
}

/** 只把能够对应真实 LiteratureReference 的正文标记转换成可点击来源。 */
export function remarkLiteratureCitations(references: readonly LiteratureReference[]) {
  return () => (tree: unknown) => replaceCitations(tree as MarkdownNode, references);
}

