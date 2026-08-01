import ReactMarkdown, { type Components } from "react-markdown";
import rehypeKatex from "rehype-katex";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import type { PluggableList } from "unified";
import "katex/dist/katex.min.css";
import { SAFE_CHAT_HTML_SCHEMA, safeMarkdownUrlTransform } from "../utils/htmlSecurity";

type MathMarkdownContentProps = {
  content: string;
  components: Components;
};

const rehypePlugins: PluggableList = [
  rehypeRaw,
  [rehypeSanitize, SAFE_CHAT_HTML_SCHEMA],
  rehypeKatex,
];

/** 公式消息才动态加载本模块，避免 KaTeX 进入普通 Chat 的常驻执行包。 */
export function MathMarkdownContent({ content, components }: MathMarkdownContentProps) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={rehypePlugins}
      components={components}
      disallowedElements={["img"]}
      urlTransform={safeMarkdownUrlTransform}
    >
      {content}
    </ReactMarkdown>
  );
}

export default MathMarkdownContent;
