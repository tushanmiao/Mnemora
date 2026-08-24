import { useEffect, useRef, useState } from "react";
import { Check, Code2, Copy, Eye, LoaderCircle, Maximize2, RotateCcw, X } from "lucide-react";
import { useImageViewer } from "../../image-viewer/ImageViewerContext";
import { useElementVisibility } from "../hooks/useElementVisibility";
import {
  createMermaidPreviewSource,
  mountMermaidSvg,
  syncMermaidOverflow,
} from "../utils/mermaidDom";
import { renderMermaid } from "../utils/mermaidRuntime";
import {
  mermaidThemeConfig,
  prepareMermaidSource,
  sanitizeMermaidSvg,
} from "../utils/mermaidSecurity";
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
  const { openImage } = useImageViewer();
  const [status, setStatus] = useState<Status>("source");
  const [svg, setSvg] = useState("");
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [showSource, setShowSource] = useState(false);
  const [retryKey, setRetryKey] = useState(0);
  const [themeRevision, setThemeRevision] = useState(0);
  const [overflowed, setOverflowed] = useState(false);
  const surfaceRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const shell = ref.current?.closest<HTMLElement>(".app-shell");
    if (!shell || typeof MutationObserver === "undefined") return;
    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      if (visible) setThemeRevision((value) => value + 1);
    });
    observer.observe(shell, { attributes: true, attributeFilter: ["data-theme", "data-theme-preset", "data-theme-color"] });
    if (root !== shell) observer.observe(root, { attributes: true, attributeFilter: ["style", "class", "data-theme"] });
    return () => observer.disconnect();
  }, [ref, visible]);

  useEffect(() => {
    let cancelled = false;
    if (streaming) {
      setStatus("source");
      setSvg("");
      return () => { cancelled = true; };
    }
    if (!visible) {
      setSvg("");
      setOverflowed(false);
      setStatus("source");
      return () => { cancelled = true; };
    }
    if (code.length > MARKDOWN_RENDER_LIMITS.maxMermaidChars) {
      setStatus("error");
      setError("图表内容过长，已保留源代码。");
      return () => { cancelled = true; };
    }

    const host = ref.current;
    if (!host) return () => { cancelled = true; };

    let prepared: string;
    try {
      prepared = prepareMermaidSource(code);
    } catch (reason) {
      setSvg("");
      setStatus("error");
      setError(reason instanceof Error ? reason.message : "Mermaid 源代码未通过安全检查。");
      return () => { cancelled = true; };
    }

    if (!prepared) {
      setSvg("");
      setStatus("source");
      return () => { cancelled = true; };
    }

    setStatus("loading");
    setError("");
    setOverflowed(false);
    const currentId = `mnemora-mermaid-${++renderSequence}`;
    const timer = window.setTimeout(() => {
      if (cancelled) return;
      void renderMermaid(prepared, currentId, mermaidThemeConfig(host, prepared)).then((result) => {
        if (cancelled) return;
        const sanitized = sanitizeMermaidSvg(result.svg);
        if (!sanitized.metrics.viewerSafe) {
          setSvg("");
          setStatus("error");
          setError("图表复杂度或尺寸超出安全限制，已保留源代码。建议按主题拆分图表。");
          return;
        }
        setSvg(sanitized.svg);
        setStatus("ready");
      }).catch((reason: unknown) => {
        if (cancelled) return;
        setSvg("");
        setStatus("error");
        setError(reason instanceof Error ? reason.message : "Mermaid 图表解析失败。");
      });
    }, MERMAID_RENDER_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [code, ref, retryKey, streaming, themeRevision, visible]);

  useEffect(() => {
    const block = ref.current;
    const surface = surfaceRef.current;
    if (showSource || status !== "ready" || !block || !surface || !svg) return;

    let mounted: SVGSVGElement;
    try {
      mounted = mountMermaidSvg(surface, svg);
    } catch (reason) {
      setStatus("error");
      setError(reason instanceof Error ? reason.message : "Mermaid SVG 显示失败。");
      return;
    }

    let frame: number | null = null;
    const measure = () => {
      if (frame !== null) return;
      frame = window.requestAnimationFrame(() => {
        frame = null;
        setOverflowed(syncMermaidOverflow(block, surface, mounted));
      });
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(surface);
    if (block.parentElement) observer?.observe(block.parentElement);

    return () => {
      observer?.disconnect();
      if (frame !== null) window.cancelAnimationFrame(frame);
      block.removeAttribute("data-mermaid-overflow");
      surface.replaceChildren();
    };
  }, [ref, showSource, status, svg]);

  const copySource = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_400);
    } catch {
      setCopied(false);
    }
  };

  const openDiagram = () => {
    const surface = surfaceRef.current;
    const mounted = surface?.querySelector<SVGSVGElement>(":scope > svg");
    if (!surface || !mounted || !overflowed) return;
    openImage({
      src: createMermaidPreviewSource(mounted, surface),
      alt: "Mermaid 图表",
      title: "Mermaid 图表",
      downloadFileName: "mermaid-diagram.svg",
    });
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
          {status === "ready" && overflowed && !showSource ? (
            <button
              type="button"
              className="markdown-enhanced-action"
              title="打开 Mermaid 图表"
              aria-label="打开 Mermaid 图表"
              aria-haspopup="dialog"
              onClick={openDiagram}
            >
              <Maximize2 size={14} />
            </button>
          ) : null}
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
        <div
          ref={surfaceRef}
          className="markdown-mermaid-surface"
          role="img"
          aria-label="Mermaid 图表"
          tabIndex={overflowed ? 0 : -1}
        />
      ) : null}
    </div>
  );
}

export default MermaidBlock;
