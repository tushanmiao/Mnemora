import { useEffect, useState } from "react";
import { Check, Code2, Copy, Eye, LoaderCircle, RotateCcw, X } from "lucide-react";
import { useElementVisibility } from "../hooks/useElementVisibility";
import { renderMermaid } from "../utils/mermaidRuntime";
import { sanitizeMermaidSvg, mermaidThemeConfig } from "../utils/mermaidSecurity";
import { MARKDOWN_RENDER_LIMITS } from "../utils/renderLimits";
import "../styles/enhanced-markdown.css";

type MermaidBlockProps = {
  code: string;
  streaming?: boolean;
};

type Status = "source" | "loading" | "ready" | "error";

let renderSequence = 0;
const MERMAID_RENDER_DEBOUNCE_MS = 120;

export function MermaidBlock({ code, streaming = false }: MermaidBlockProps) {
  const { ref, visible } = useElementVisibility<HTMLDivElement>();
  const [status, setStatus] = useState<Status>("source");
  const [svg, setSvg] = useState("");
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [showSource, setShowSource] = useState(false);
  const [retryKey, setRetryKey] = useState(0);
  const [themeRevision, setThemeRevision] = useState(0);
  const [expanded, setExpanded] = useState(false);
  const isLarge = code.length > 8_000 || code.split(/\r?\n/).length > 64;

  useEffect(() => {
    const shell = ref.current?.closest<HTMLElement>(".app-shell");
    if (!shell || typeof MutationObserver === "undefined") return;
    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      if (visible) setThemeRevision((value) => value + 1);
    });
    observer.observe(shell, { attributes: true, attributeFilter: ["data-theme", "data-theme-preset", "data-theme-color"] });
    if (root !== shell) {
      observer.observe(root, { attributes: true, attributeFilter: ["style", "class", "data-theme"] });
    }
    return () => observer.disconnect();
  }, [ref, visible]);

  useEffect(() => {
    let cancelled = false;
    if (streaming || showSource) return () => { cancelled = true; };
    if (!visible) {
      // 离开可视区后释放 SVG 字符串，避免长对话持续保留图形 DOM。
      setSvg((current) => current ? "" : current);
      setStatus((current) => current === "ready" ? "source" : current);
      return () => { cancelled = true; };
    }
    if (code.length > MARKDOWN_RENDER_LIMITS.maxMermaidChars) {
      setStatus("error");
      setError("图表内容过长，已保留源代码。 ");
      return () => { cancelled = true; };
    }

    setStatus("loading");
    setError("");
    const host = ref.current;
    if (!host) return () => { cancelled = true; };
    const currentId = `mnemora-mermaid-${++renderSequence}`;
    const timer = window.setTimeout(() => {
      if (cancelled) return;
      // A disclosure panel may have become visible in this frame. Measuring
      // once makes its current width available before Mermaid lays out SVG.
      void host.getBoundingClientRect();
      void renderMermaid(code, currentId, mermaidThemeConfig(host)).then((result) => {
        if (cancelled) return;
        setSvg(sanitizeMermaidSvg(result.svg));
        setStatus("ready");
      }).catch((reason: unknown) => {
        if (cancelled) return;
        setSvg("");
        setStatus("error");
        setError(reason instanceof Error ? reason.message : "Mermaid 图表解析失败");
      });
    }, MERMAID_RENDER_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [code, ref, retryKey, showSource, streaming, themeRevision, visible]);

  const copySource = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_400);
    } catch {
      setCopied(false);
    }
  };

  const renderButton = status === "ready" && !showSource ? (
    <button type="button" className="markdown-enhanced-action" title="查看 Mermaid 源代码" aria-label="查看 Mermaid 源代码" onClick={() => setShowSource(true)}>
      <Code2 size={14} />
    </button>
  ) : (
    <button type="button" className="markdown-enhanced-action" title="显示 Mermaid 图表" aria-label="显示 Mermaid 图表" onClick={() => setShowSource(false)}>
      <Eye size={14} />
    </button>
  );

  return (
    <div ref={ref} className="markdown-mermaid-block">
      <div className="markdown-code-toolbar">
        <span>mermaid</span>
        <div className="markdown-code-actions">
          {renderButton}
          <button type="button" className="markdown-enhanced-action" title={copied ? "已复制" : "复制 Mermaid 源代码"} aria-label={copied ? "已复制" : "复制 Mermaid 源代码"} onClick={() => void copySource()}>
            {copied ? <Check size={14} /> : <Copy size={14} />}
          </button>
          {status === "error" ? (
            <button type="button" className="markdown-enhanced-action" title="重试 Mermaid 渲染" aria-label="重试 Mermaid 渲染" onClick={() => { setShowSource(false); setRetryKey((value) => value + 1); }}>
              <RotateCcw size={14} />
            </button>
          ) : null}
        </div>
      </div>
      {showSource || status === "source" || status === "loading" || status === "error" ? (
        <>
          {status === "loading" && !showSource ? <div className="markdown-enhanced-status"><LoaderCircle className="message-spin" size={15} />正在生成图表…</div> : null}
          {status === "error" && !showSource ? <div className="markdown-enhanced-error"><X size={15} /><span>{error}</span></div> : null}
          {showSource || status === "source" || status === "error" ? <pre className="markdown-mermaid-source"><code>{code}</code></pre> : null}
        </>
      ) : null}
      {status === "ready" && !showSource ? (
        <>
          <div className={`markdown-mermaid-svg${isLarge && !expanded ? " markdown-mermaid-collapsed" : ""}`} dangerouslySetInnerHTML={{ __html: svg }} />
          {isLarge ? <button type="button" className="markdown-code-expand" onClick={() => setExpanded((value) => !value)}>{expanded ? "收起图表" : "展开完整图表"}</button> : null}
        </>
      ) : null}
    </div>
  );
}

export default MermaidBlock;
