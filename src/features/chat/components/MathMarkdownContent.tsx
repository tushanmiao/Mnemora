import ReactMarkdown, { type Components } from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkMath from "remark-math";
import "katex/dist/katex.min.css";
import { safeMarkdownContentUrlTransform } from "../utils/htmlSecurity";
import type { LiteratureReference } from "../../../types/chat";
import { createMarkdownRehypePlugins, createMarkdownRemarkPlugins } from "../markdown/plugins/markdownPlugins";

type MathMarkdownContentProps = {
  content: string;
  components: Components;
  messageId?: string;
  literatureReferences?: readonly LiteratureReference[];
};

/** 公式消息才动态加载本模块，避免 KaTeX 进入普通 Chat 的常驻执行包。 */
export function MathMarkdownContent({ content, components, messageId = "message", literatureReferences = [] }: MathMarkdownContentProps) {
  return (
    <ReactMarkdown
      remarkPlugins={[...createMarkdownRemarkPlugins(literatureReferences), remarkMath]}
      rehypePlugins={[...createMarkdownRehypePlugins(messageId), rehypeKatex]}
      components={components}
      urlTransform={safeMarkdownContentUrlTransform}
    >
      {content}
    </ReactMarkdown>
  );
}

export default MathMarkdownContent;
