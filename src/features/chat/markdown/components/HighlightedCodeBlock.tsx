import { useEffect, useState, type ReactNode } from "react";
import { Check, Code2, Copy, Eye } from "lucide-react";
import { useElementVisibility } from "../hooks/useElementVisibility";
import { MARKDOWN_RENDER_LIMITS } from "../utils/renderLimits";
import { normalizeCodeLanguage } from "../utils/codeBlock";
import "../styles/enhanced-markdown.css";

type HighlightedCodeBlockProps = {
  code: string;
  language: string | null;
  previewContent?: ReactNode;
  previewNotice?: string;
};

const languageLoaders: Record<string, () => Promise<unknown>> = {
  javascript: () => import("highlight.js/lib/languages/javascript"),
  typescript: () => import("highlight.js/lib/languages/typescript"),
  jsx: () => import("highlight.js/lib/languages/xml"),
  tsx: () => import("highlight.js/lib/languages/typescript"),
  rust: () => import("highlight.js/lib/languages/rust"),
  python: () => import("highlight.js/lib/languages/python"),
  java: () => import("highlight.js/lib/languages/java"),
  c: () => import("highlight.js/lib/languages/c"),
  cpp: () => import("highlight.js/lib/languages/cpp"),
  csharp: () => import("highlight.js/lib/languages/csharp"),
  go: () => import("highlight.js/lib/languages/go"),
  sql: () => import("highlight.js/lib/languages/sql"),
  pgsql: () => import("highlight.js/lib/languages/pgsql"),
  json: () => import("highlight.js/lib/languages/json"),
  yaml: () => import("highlight.js/lib/languages/yaml"),
  shell: () => import("highlight.js/lib/languages/shell"),
  powershell: () => import("highlight.js/lib/languages/powershell"),
  html: () => import("highlight.js/lib/languages/xml"),
  css: () => import("highlight.js/lib/languages/css"),
  markdown: () => import("highlight.js/lib/languages/markdown"),
};

const registeredLanguages = new Set<string>();

async function highlightCode(code: string, language: string) {
  const core = await import("highlight.js/lib/core");
  const loader = languageLoaders[language];
  if (!loader) return null;
  if (!registeredLanguages.has(language)) {
    const module = await loader() as { default?: (hljs: unknown) => unknown };
    if (module.default) core.default.registerLanguage(language, module.default as never);
    registeredLanguages.add(language);
  }
  return core.default.highlight(code, { language, ignoreIllegals: true }).value;
}

export function HighlightedCodeBlock({ code, language, previewContent, previewNotice }: HighlightedCodeBlockProps) {
  const normalized = normalizeCodeLanguage(language);
  const { ref, visible } = useElementVisibility<HTMLDivElement>();
  const [html, setHtml] = useState("");
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [source, setSource] = useState(false);
  const [previewing, setPreviewing] = useState(() => Boolean(previewContent));
  const [expanded, setExpanded] = useState(false);
  const isLong = code.split(/\r?\n/).length > MARKDOWN_RENDER_LIMITS.maxLongCodeLines;

  useEffect(() => {
    let cancelled = false;
    setHtml("");
    setError("");
    if (!visible || !normalized || normalized === "text" || !languageLoaders[normalized]) return () => { cancelled = true; };
    if (code.length > MARKDOWN_RENDER_LIMITS.maxHighlightedCodeChars) {
      setError("代码内容过长，已使用纯文本显示。");
      return () => { cancelled = true; };
    }
    void highlightCode(code, normalized).then((value) => {
      if (!cancelled && value !== null) setHtml(value);
    }).catch((reason: unknown) => {
      if (!cancelled) setError(reason instanceof Error ? reason.message : "代码高亮失败");
    });
    return () => { cancelled = true; };
  }, [code, normalized, visible]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_400);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="markdown-highlighted-code" ref={ref}>
      <div className="markdown-code-toolbar">
        <span>{language ?? "代码"}</span>
        <div className="markdown-code-actions">
          {previewContent ? <button type="button" className="markdown-enhanced-action" title={previewing ? "查看 Markdown 源码" : "渲染内部 Markdown"} aria-label={previewing ? "查看 Markdown 源码" : "渲染内部 Markdown"} onClick={() => setPreviewing((current) => !current)}>{previewing ? <Code2 size={14} /> : <Eye size={14} />}</button> : null}
          {html && !previewContent ? <button type="button" className="markdown-enhanced-action" title={source ? "显示高亮" : "显示源代码"} aria-label={source ? "显示高亮" : "显示源代码"} onClick={() => setSource((current) => !current)}>{source ? <Eye size={14} /> : <Code2 size={14} />}</button> : null}
          <button type="button" className="markdown-enhanced-action" title={copied ? "已复制" : "复制代码"} aria-label={copied ? "已复制" : "复制代码"} onClick={() => void copy()}>{copied ? <Check size={14} /> : <Copy size={14} />}</button>
        </div>
      </div>
      {error && !previewing ? <div className="markdown-enhanced-error">{error}</div> : null}
      {previewNotice ? <div className="markdown-source-nesting-warning">{previewing ? "检测到 Markdown 源码块中的 Mermaid，已按安全预览渲染；点击代码图标可查看原始 Markdown。" : previewNotice}</div> : null}
      {previewing ? <div className="markdown-source-preview">{previewContent}</div> : !html ? <pre className={isLong && !expanded ? "markdown-code-collapsed" : undefined}><code>{code}</code></pre> : source ? <pre className={isLong && !expanded ? "markdown-code-collapsed" : undefined}><code>{code}</code></pre> : <pre className={isLong && !expanded ? "hljs markdown-code-collapsed" : "hljs"} dangerouslySetInnerHTML={{ __html: html }} />}
      {isLong && !previewing ? <button type="button" className="markdown-code-expand" onClick={() => setExpanded((value) => !value)}>{expanded ? "收起代码" : "展开完整代码"}</button> : null}
    </div>
  );
}
