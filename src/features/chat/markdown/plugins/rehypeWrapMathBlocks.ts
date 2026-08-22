type HastNode = {
  type?: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  data?: Record<string, unknown>;
  children?: HastNode[];
};

export type MathBlockSource = {
  latex: string;
  display: true;
};

export function isMathBlockNode(node: HastNode): node is HastNode & { data: { mnemoraMathBlock: true; latexSource: string } } {
  return Boolean(node?.data?.mnemoraMathBlock === true && typeof node.data.latexSource === "string");
}

export function rehypeWrapMathBlocksForTest(tree: HastNode) {
  wrapMathChildren(tree);
  return tree;
}

/**
 * 在 KaTeX 转换前保存独立公式的原始 LaTeX，并增加稳定的 UI 容器。
 * rehype-katex 会替换内部 pre/code，但保留外层 div；即使公式解析失败，
 * 用户仍然可以查看和复制原始公式。
 */
export function rehypeWrapMathBlocks() {
  return (tree: HastNode) => wrapMathChildren(tree);
}

function wrapMathChildren(parent: HastNode) {
  const children = parent.children;
  if (!children) return;

  for (let index = 0; index < children.length; index += 1) {
    const child = children[index];
    const code = getMathCodeNode(child);
    if (code) {
      const latex = readText(code).replace(/\r?\n$/, "");
      children[index] = {
        type: "element",
        tagName: "div",
        properties: {},
        data: {
          mnemoraMathBlock: true,
          latexSource: latex,
        },
        children: [child],
      };
      continue;
    }
    wrapMathChildren(child);
  }
}

function getMathCodeNode(node: HastNode) {
  if (node.type !== "element" || node.tagName !== "pre") return null;
  const code = node.children?.[0];
  if (code?.type !== "element" || code.tagName !== "code") return null;
  const classes = Array.isArray(code.properties?.className)
    ? code.properties.className.map(String)
    : String(code.properties?.className ?? "").split(/\s+/);
  return classes.includes("math-display") || classes.includes("language-math") ? code : null;
}

function readText(node: HastNode): string {
  if (node.type === "text") return node.value ?? "";
  return (node.children ?? []).map(readText).join("");
}
