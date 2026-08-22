import ReactMarkdown, { type Components } from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkMath from "remark-math";
import "katex/dist/katex.min.css";
import { safeMarkdownContentUrlTransform } from "../utils/htmlSecurity";
import type { LiteratureReference } from "../../../types/chat";
import { createMarkdownRehypePlugins, createMarkdownRemarkPlugins } from "../markdown/plugins/markdownPlugins";
import { rehypeWrapMathBlocks } from "../markdown/plugins/rehypeWrapMathBlocks";
import { MathFormulaBlock } from "./MathFormulaBlock";
import "../styles/math-formula.css";

type MathMarkdownContentProps = {
  content: string;
  components: Components;
  messageId?: string;
  literatureReferences?: readonly LiteratureReference[];
};

/** 公式消息才动态加载本模块，避免 KaTeX 进入普通 Chat 的常驻执行包。 */
export function MathMarkdownContent({ content, components, messageId = "message", literatureReferences = [] }: MathMarkdownContentProps) {
  const mathComponents: Components = {
    ...components,
    div({ node, ...props }) {
      const mathNode = node as unknown as { data?: { mnemoraMathBlock?: boolean; latexSource?: string } } | undefined;
      if (mathNode?.data?.mnemoraMathBlock === true && typeof mathNode.data.latexSource === "string") {
        const latex = mathNode.data.latexSource;
        return <MathFormulaBlock latex={latex}>{props.children}</MathFormulaBlock>;
      }
      return <div {...props} />;
    },
  };
  return (
    <ReactMarkdown
      remarkPlugins={[...createMarkdownRemarkPlugins(literatureReferences), remarkMath]}
      rehypePlugins={[...createMarkdownRehypePlugins(messageId), rehypeWrapMathBlocks, rehypeKatex]}
      components={mathComponents}
      urlTransform={safeMarkdownContentUrlTransform}
    >
      {content}
    </ReactMarkdown>
  );
}

export default MathMarkdownContent;
