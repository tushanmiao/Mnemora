type MarkdownNode = {
  type: string;
  value?: string;
  children?: MarkdownNode[];
  data?: {
    hName?: string;
    hProperties?: Record<string, unknown>;
  };
};

const CALLOUT_TYPES = new Set([
  "note", "tip", "important", "warning", "definition", "example", "evidence", "question",
]);

function visit(node: MarkdownNode) {
  if (node.type === "blockquote") {
    const paragraph = node.children?.[0];
    const firstText = paragraph?.children?.find((child) => child.type === "text" && child.value);
    const marker = firstText?.value?.match(/^\[!([A-Za-z]+)\][ \t]*(?:\n|$)/);
    const type = marker?.[1].toLowerCase();
    if (firstText?.value && marker && type && CALLOUT_TYPES.has(type)) {
      firstText.value = firstText.value.slice(marker[0].length);
      if (!firstText.value && paragraph) {
        paragraph.children = paragraph.children?.filter((child) => child !== firstText);
      }
      node.data = {
        ...(node.data ?? {}),
        hName: "aside",
        hProperties: { ...(node.data?.hProperties ?? {}), dataCallout: type },
      };
    }
  }
  node.children?.forEach(visit);
}

/** 将 GitHub/Obsidian 风格的 [!NOTE] 引用转换为受控提示块。 */
export function remarkLearningCallouts() {
  return (tree: unknown) => visit(tree as MarkdownNode);
}
