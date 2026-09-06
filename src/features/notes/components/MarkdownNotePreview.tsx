import { useNoteText } from "../editor/noteText";
import { useMemo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import { convertFileSrc } from "@tauri-apps/api/core";
import { openNoteLink } from "../editor/noteLinks";
import remarkMath from "remark-math";
import remarkFrontmatter from "remark-frontmatter";
import rehypeKatex from "rehype-katex";
import rehypeRaw from "rehype-raw";
import rehypeSanitize, { type Options as SanitizeSchema } from "rehype-sanitize";
import { createMarkdownRemarkPlugins } from "../../chat/markdown/plugins/markdownPlugins";
import { rehypeScopeDocument } from "../../chat/markdown/plugins/rehypeScopeDocument";
import { SAFE_CHAT_HTML_SCHEMA } from "../../chat/utils/htmlSecurity";
import { HighlightedCodeBlock } from "../../chat/markdown/components/HighlightedCodeBlock";
import { MermaidBlock } from "../../chat/markdown/components/MermaidBlock";
import { SafeMarkdownImage } from "../../chat/markdown/components/SafeMarkdownImage";
import { LearningCallout } from "../../chat/markdown/components/LearningCallout";
import { extractCodeLanguage, extractCodeText } from "../../chat/markdown/utils/codeBlock";
import { RenderFallback } from "../../chat/markdown/components/RenderFallback";
import { createSafeNoteMarkdownUrlTransform } from "../utils/noteMarkdownUrls";
import { remarkNoteHighlight } from "../editor/remarkNoteHighlight";
import { remarkNoteMath } from "../editor/remarkNoteMath";
import { rehypeNoteLinks } from "../editor/rehypeNoteLinks";
import "katex/dist/katex.min.css";
import "../../chat/styles/markdown-message.css";
import "../../chat/markdown/styles/enhanced-markdown.css";

export const noteHtmlSchema: SanitizeSchema = {
  ...SAFE_CHAT_HTML_SCHEMA,
  tagNames: [...SAFE_CHAT_HTML_SCHEMA.tagNames!, "u", "sub", "mark", "input"],
  strip: SAFE_CHAT_HTML_SCHEMA.strip!.filter((tag) => tag !== "input"),
  attributes: { ...SAFE_CHAT_HTML_SCHEMA.attributes,
    input: [["type", "checkbox"], ["disabled", true], "checked"],
    td: ["colSpan", "rowSpan", "align"], th: ["colSpan", "rowSpan", "align"],
  },
};

type MarkdownNotePreviewProps = {
  noteId: string;
  content: string;
  directoryPath?: string | null;
  fragment?: boolean;
};

export default function MarkdownNotePreview({ noteId, content, directoryPath, fragment }: MarkdownNotePreviewProps) {
  const nt = useNoteText();
  const assetBaseUrl = directoryPath ? convertFileSrc(directoryPath) : null;
  const plugins = useMemo(() => [rehypeRaw, rehypeScopeDocument(`note-${noteId}`), rehypeNoteLinks,
    [rehypeSanitize, noteHtmlSchema], [rehypeKatex, { trust: false, maxExpand: 1000, maxSize: 20, strict: "warn", throwOnError: false }]] as import("unified").PluggableList, [noteId]);
  const components = useMemo<Components>(() => ({
    a({ node, ...props }) {
      void node;
      return <a {...props} target={props.href?.startsWith("#") ? undefined : "_blank"} rel="noopener noreferrer" onClick={(event) => {
        const href = props.href ?? "";
        if (href.startsWith("#")) { event.preventDefault(); document.getElementById(href.slice(1))?.scrollIntoView({ block: "center" }); }
        else if (href.startsWith("mnemora-citation:")) event.preventDefault();
        else { event.preventDefault(); void openNoteLink(noteId, href).catch((error: unknown) => {
          event.currentTarget?.setAttribute("title", String(error));
        }); }
      }} />;
    },
    input({ node, ...props }) { void node; return <input {...props} type="checkbox" disabled readOnly />; },
    img({ node, ...props }) { void node; return <SafeMarkdownImage {...props} />; },
    aside({ node, ...props }) { void node; return <LearningCallout {...props} />; },
    table({ node, ...props }) { void node; return <div className="markdown-table-scroll"><table {...props} /></div>; },
    pre({ children }) {
      const code = extractCodeText(children).replace(/\n$/, ""), language = extractCodeLanguage(children);
      if (language?.toLowerCase() === "mermaid") return <MermaidBlock code={code} />;
      return <HighlightedCodeBlock code={code} language={language ?? "text"} />;
    },
  }), [noteId]);
  const frontmatter = content.match(/^---\r?\n([\s\S]*?)\r?\n(?:---|\.\.\.)(?:\r?\n|$)/)?.[1];
  return <article className="notes-markdown-preview" aria-label={fragment ? nt("Markdown 块预览") : nt("Markdown 阅读")}>
    <div className="markdown-content">
      {frontmatter ? <details className="note-frontmatter"><summary>Front matter</summary><pre>{frontmatter}</pre></details> : null}
      <RenderFallback fallback={<pre>{content}</pre>}>
        <ReactMarkdown
          remarkPlugins={[...createMarkdownRemarkPlugins([]), remarkMath, remarkFrontmatter, remarkNoteMath, remarkNoteHighlight]}
          rehypePlugins={plugins} components={components} urlTransform={createSafeNoteMarkdownUrlTransform(assetBaseUrl)}
        >{content}</ReactMarkdown>
      </RenderFallback>
    </div>
  </article>;
}
