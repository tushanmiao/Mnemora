import { describe, expect, it } from "vitest";
import { isMathBlockNode, rehypeWrapMathBlocksForTest } from "./rehypeWrapMathBlocks";

describe("rehypeWrapMathBlocks", () => {
  it("captures display math source before KaTeX transforms the node", () => {
    const tree = {
      type: "root",
      children: [{
        type: "element",
        tagName: "pre",
        children: [{
          type: "element",
          tagName: "code",
          properties: { className: ["language-math", "math-display"] },
          children: [{ type: "text", value: "\\frac{1}{2}\n" }],
        }],
      }],
    } as unknown as {
      children: Array<{ data?: { latexSource?: string; mnemoraMathBlock?: boolean } }>;
    };

    rehypeWrapMathBlocksForTest(tree);
    const wrapper = tree.children[0];
    expect(isMathBlockNode(wrapper)).toBe(true);
    expect(wrapper.data?.latexSource).toBe("\\frac{1}{2}");
  });
});
