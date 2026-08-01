import type { Options as SanitizeSchema } from "rehype-sanitize";

const SAFE_HTML_TAGS = [
  "a",
  "aside",
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
  "img",
  "li",
  "ol",
  "p",
  "pre",
  "span",
  "strong",
  "sup",
  "section",
  "summary",
  "details",
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
    "*": [
      ["id", /^(?:mnemora-heading|mnemora-doc)-[a-zA-Z0-9_-]+$/],
    ],
    a: ["href", "title", "ariaLabel", "ariaDescribedBy", "dataFootnoteRef", "dataFootnoteBackref"],
    code: [["className", /^language-[a-z0-9_-]+$/i]],
    details: ["open"],
    img: ["alt", "height", "loading", "src", "title", "width"],
    section: ["dataFootnotes"],
    aside: [["dataCallout", /^(note|tip|important|warning|definition|example|evidence|question)$/]],
    td: ["colSpan", "rowSpan"],
    th: ["colSpan", "rowSpan"],
  },
  protocols: {
    href: ["http", "https", "mailto", "mnemora-citation"],
    src: ["https", "asset", "blob"],
  },
  clobber: [],
  strip: [
    "audio",
    "base",
    "button",
    "canvas",
    "embed",
    "form",
    "iframe",
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
  if (value.startsWith("#")) return value;
  if (value.startsWith("mnemora-citation:")) return value;
  try {
    const url = new URL(value);
    return ["http:", "https:", "mailto:", "asset:", "blob:", "mnemora-citation:"].includes(url.protocol) ? value : "";
  } catch {
    return "";
  }
}

/** 图片比普通链接更严格，不允许 data URL 和本地 file URL 进入消息 DOM。 */
export function safeMarkdownImageUrlTransform(value: string) {
  try {
    const url = new URL(value);
    return ["https:", "asset:", "blob:"].includes(url.protocol) ? value : "";
  } catch {
    return "";
  }
}

export function safeMarkdownContentUrlTransform(value: string, key: string) {
  return key === "src" ? safeMarkdownImageUrlTransform(value) : safeMarkdownUrlTransform(value);
}
