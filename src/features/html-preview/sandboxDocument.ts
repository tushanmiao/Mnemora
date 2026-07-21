import rehypeParse from "rehype-parse";
import rehypeSanitize, { type Options as SanitizeSchema } from "rehype-sanitize";
import rehypeStringify from "rehype-stringify";
import { unified } from "unified";

const PREVIEW_HTML_SCHEMA: SanitizeSchema = {
  allowComments: false,
  allowDoctypes: false,
  tagNames: [
    "a", "abbr", "article", "aside", "b", "blockquote", "body", "br", "caption", "cite",
    "code", "col", "colgroup", "dd", "del", "details", "dfn", "div", "dl", "dt", "em",
    "figcaption", "figure", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header",
    "hr", "html", "i", "img", "ins", "kbd", "li", "main", "mark", "nav", "ol", "p", "pre",
    "q", "s", "samp", "section", "small", "span", "strong", "style", "sub", "summary", "sup",
    "table", "tbody", "td", "tfoot", "th", "thead", "time", "title", "tr", "u", "ul", "var",
  ],
  attributes: {
    "*": ["className", "dir", "hidden", "id", "lang", "style", "title"],
    col: ["span"],
    img: ["alt", "height", "src", "width"],
    ol: ["reversed", "start", "type"],
    td: ["colSpan", "rowSpan"],
    th: ["colSpan", "rowSpan", "scope"],
    time: ["dateTime"],
  },
  protocols: {
    src: ["data"],
  },
  clobber: [],
  strip: [
    "audio", "base", "button", "canvas", "embed", "form", "iframe", "input", "link", "meta",
    "object", "script", "select", "svg", "textarea", "video",
  ],
};

const PREVIEW_CSP = [
  "default-src 'none'",
  "base-uri 'none'",
  "connect-src 'none'",
  "font-src data:",
  "form-action 'none'",
  "frame-src 'none'",
  "img-src data:",
  "media-src 'none'",
  "object-src 'none'",
  "script-src 'none'",
  "style-src 'unsafe-inline'",
].join("; ");

const TRUSTED_PREVIEW_HEAD = [
  `<meta http-equiv="Content-Security-Policy" content="${PREVIEW_CSP}">`,
  '<meta name="referrer" content="no-referrer">',
  '<meta name="viewport" content="width=device-width, initial-scale=1">',
  '<style>html{color-scheme:light dark}body{margin:24px;font-family:system-ui,-apple-system,"Segoe UI",sans-serif;line-height:1.55;overflow-wrap:anywhere}img{max-width:100%;height:auto}table{max-width:100%;border-collapse:collapse}th,td{padding:6px 8px;border:1px solid #8888}pre{max-width:100%;overflow:auto;white-space:pre}code{font-family:"Cascadia Code",Consolas,monospace}</style>',
].join("");

const previewProcessor = unified()
  .use(rehypeParse)
  .use(rehypeSanitize, PREVIEW_HTML_SCHEMA)
  .use(rehypeStringify);

/**
 * 预览文档先经过独立白名单清洗，再写入最严格的文档级 CSP。
 * iframe 本身不授予 allow-scripts、allow-same-origin、allow-forms 等任何 sandbox 权限。
 */
export function buildSandboxDocument(source: string) {
  const sanitized = String(previewProcessor.processSync(source));
  if (/<head(?:\s[^>]*)?>/i.test(sanitized)) {
    return sanitized.replace(/<head(?:\s[^>]*)?>/i, (head) => `${head}${TRUSTED_PREVIEW_HEAD}`);
  }
  if (/<html(?:\s[^>]*)?>/i.test(sanitized)) {
    return sanitized.replace(/<html(?:\s[^>]*)?>/i, (html) => `${html}<head>${TRUSTED_PREVIEW_HEAD}</head>`);
  }
  return `<!doctype html><html><head>${TRUSTED_PREVIEW_HEAD}</head><body>${sanitized}</body></html>`;
}

