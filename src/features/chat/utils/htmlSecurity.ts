import type { Options as SanitizeSchema } from "rehype-sanitize";

const SAFE_HTML_TAGS = [
  "a",
  "blockquote",
  "br",
  "code",
  "del",
  "div",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "li",
  "ol",
  "p",
  "pre",
  "span",
  "strong",
  "table",
  "tbody",
  "td",
  "th",
  "thead",
  "tr",
  "ul",
] as const;

/**
 * 聊天正文只允许静态排版标签。原始 HTML 的 class、style、id、事件属性和媒体元素均会被移除。
 * code 的 language-* 类名由 Markdown 围栏生成，仅用于识别代码语言和提供 HTML 预览入口。
 */
export const SAFE_CHAT_HTML_SCHEMA: SanitizeSchema = {
  allowComments: false,
  allowDoctypes: false,
  tagNames: [...SAFE_HTML_TAGS],
  attributes: {
    a: ["href", "title"],
    code: [["className", /^language-[a-z0-9_-]+$/i]],
    td: ["colSpan", "rowSpan"],
    th: ["colSpan", "rowSpan"],
  },
  protocols: {
    href: ["http", "https", "mailto"],
  },
  clobber: ["id", "name"],
  clobberPrefix: "mnemora-user-content-",
  strip: [
    "audio",
    "base",
    "button",
    "canvas",
    "embed",
    "form",
    "iframe",
    "img",
    "input",
    "link",
    "meta",
    "object",
    "script",
    "select",
    "style",
    "svg",
    "textarea",
    "video",
  ],
};

/** 聊天链接只允许交给系统浏览器处理的绝对外部地址。 */
export function safeMarkdownUrlTransform(value: string) {
  try {
    const url = new URL(value);
    return ["http:", "https:", "mailto:"].includes(url.protocol) ? value : "";
  } catch {
    return "";
  }
}
