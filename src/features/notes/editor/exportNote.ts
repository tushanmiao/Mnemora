import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkFrontmatter from "remark-frontmatter";
import remarkRehype from "remark-rehype";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import rehypeKatex from "rehype-katex";
import rehypeStringify from "rehype-stringify";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { noteHtmlSchema } from "../components/MarkdownNotePreview";
import { remarkNoteHighlight } from "./remarkNoteHighlight";
import { remarkNoteMath } from "./remarkNoteMath";
import { rehypeNoteLinks } from "./rehypeNoteLinks";
import { remarkLearningCallouts } from "../../chat/markdown/plugins/remarkLearningCallouts";
import { rehypeScopeDocument } from "../../chat/markdown/plugins/rehypeScopeDocument";
import { downloadNoteText } from "../components/NoteHistoryPanel";
import katexCss from "katex/dist/katex.min.css?inline";
const katexFonts = import.meta.glob<string>("/node_modules/katex/dist/fonts/*.woff2", { query: "?inline", import: "default", eager: true });

export async function exportNoteBundle(noteId: string, title: string, markdown: string) {
  const destination = await open({ directory: true, multiple: false, title: "导出笔记与附件" });
  if (!destination) return;
  await invoke("note_editor_export_bundle", { noteId, title, markdown, destination });
}

export async function buildNoteHtml(noteId: string, title: string, markdown: string, host: HTMLElement) {
  const output = await unified().use(remarkParse).use(remarkGfm).use(remarkMath).use(remarkFrontmatter).use(remarkNoteMath).use(remarkNoteHighlight).use(remarkLearningCallouts)
    .use(remarkRehype, { allowDangerousHtml: true }).use(rehypeRaw).use(rehypeScopeDocument(`note-${noteId}`)).use(rehypeNoteLinks)
    .use(rehypeSanitize, noteHtmlSchema).use(rehypeKatex, { trust: false, maxExpand: 1000, maxSize: 20, throwOnError: false })
    .use(rehypeStringify).process(markdown);
  const document = new DOMParser().parseFromString(String(output), "text/html");
  document.title = title;
  const frontmatter = markdown.match(/^---\r?\n([\s\S]*?)\r?\n(?:---|\.\.\.)(?:\r?\n|$)/)?.[1];
  if (frontmatter !== undefined) {
    const details = document.createElement("details"), summary = document.createElement("summary"), source = document.createElement("pre");
    summary.textContent = "Front matter"; source.textContent = frontmatter;
    details.append(summary, source); document.body.prepend(details);
  }
  for (const image of document.querySelectorAll("img")) {
    const src = image.getAttribute("src") ?? "";
    if (src.startsWith("attachments/")) image.src = await invoke<string>("note_editor_read_asset", { noteId, relativePath: src });
    else if (!src.startsWith("https://")) image.removeAttribute("src");
  }
  for (const input of document.querySelectorAll("input")) { input.setAttribute("disabled", ""); input.setAttribute("type", "checkbox"); }
  for (const code of document.querySelectorAll("pre > code.language-mermaid")) {
    const source = code.textContent ?? "";
    if (source.length > 24000) continue;
    try {
      const [{ renderMermaid }, security] = await Promise.all([import("../../chat/markdown/utils/mermaidRuntime"), import("../../chat/markdown/utils/mermaidSecurity")]);
      const prepared = security.prepareMermaidSource(source), paint = security.mermaidSvgPaint(host);
      const rendered = await renderMermaid(prepared, `export-${crypto.randomUUID()}`, security.mermaidThemeConfig(host, prepared, paint));
      const safe = security.sanitizeMermaidSvg(rendered.svg, paint);
      if (!safe.metrics.viewerSafe) continue;
      const svg = new DOMParser().parseFromString(safe.svg, "image/svg+xml").documentElement;
      code.parentElement!.replaceWith(document.importNode(svg, true));
    } catch { /* Invalid diagrams remain complete source in the exported document. */ }
  }
  const styles = document.createElement("style");
  const sheet = new CSSStyleSheet(); sheet.replaceSync(katexCss);
  const rules = [...sheet.cssRules].filter((rule) => rule.type !== CSSRule.FONT_FACE_RULE).map((rule) => rule.cssText).join("\n");
  const fonts = Object.entries(katexFonts).map(([path, data]) => {
    const name = path.split("/").pop()!.replace(/\.woff2$/, "");
    const [family, variant] = name.split("-");
    return `@font-face{font-family:${family};font-style:${variant.includes("Italic") ? "italic" : "normal"};font-weight:${variant.includes("Bold") ? "700" : "400"};src:url("${data}") format("woff2")}`;
  }).join("\n");
  styles.textContent = `${fonts}\n${rules}\nbody{max-width:80ch;margin:40px auto;padding:0 20px;font:16px/1.8 system-ui,sans-serif;color:#202124;background:#fff}pre{white-space:pre-wrap;overflow-wrap:anywhere;padding:12px;background:#f3f4f5}table{border-collapse:collapse;max-width:100%;display:block;overflow:auto}th,td{border:1px solid #c7c9ce;padding:6px 10px}img,svg{max-width:100%;height:auto}a{color:#165baf}mark{background:#ffe694}blockquote{margin-left:0;padding-left:16px;border-left:1px solid #aeb0b5}`;
  document.head.append(styles);
  const charset = document.createElement("meta"); charset.setAttribute("charset", "utf-8"); document.head.prepend(charset);
  const csp = document.createElement("meta"); csp.httpEquiv = "Content-Security-Policy";
  csp.content = "default-src 'none'; img-src data: https:; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'";
  document.head.append(csp);
  const viewport = document.createElement("meta"); viewport.name = "viewport"; viewport.content = "width=device-width,initial-scale=1"; document.head.append(viewport);
  return `<!doctype html>\n${document.documentElement.outerHTML}`;
}

export async function exportNoteHtml(noteId: string, title: string, markdown: string, host: HTMLElement) {
  downloadNoteText(title, await buildNoteHtml(noteId, title, markdown, host), "html", "text/html;charset=utf-8");
}
