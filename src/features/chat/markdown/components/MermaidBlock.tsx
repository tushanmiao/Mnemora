import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Check, Code2, Copy, Eye, LoaderCircle, Maximize2, Minus, Plus, RotateCcw, X } from "lucide-react";
import { useElementVisibility } from "../hooks/useElementVisibility";
import { renderMermaid } from "../utils/mermaidRuntime";
import { isLargeMermaidDiagram, sanitizeMermaidSvg, mermaidThemeConfig, type MermaidSvgMetrics } from "../utils/mermaidSecurity";
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
  const [metrics, setMetrics] = useState<MermaidSvgMetrics | null>(null);
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const panRef = useRef<{ pointerId: number; x: number; y: number; originX: number; originY: number } | null>(null);
  const lightboxRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);

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
      const width = Math.max(280, host.getBoundingClientRect().width - 32);
      void renderMermaid(code, currentId, mermaidThemeConfig(host), width).then((result) => {
        if (cancelled) return;
        const sanitized = sanitizeMermaidSvg(result.svg);
        setSvg(sanitized.svg);
        setMetrics(sanitized.metrics);
        setZoom(1);
        setPan({ x: 0, y: 0 });
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

  useEffect(() => {
    if (!expanded) return undefined;
    const previousOverflow = document.body.style.overflow;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setExpanded(false);
        setZoom(1);
        setPan({ x: 0, y: 0 });
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...(lightboxRef.current?.querySelectorAll<HTMLElement>("button:not(:disabled), [tabindex]:not([tabindex='-1'])") ?? [])];
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.body.style.overflow = "hidden";
    document.addEventListener("keydown", onKeyDown);
    window.requestAnimationFrame(() => closeButtonRef.current?.focus());
    return () => {
      document.body.style.overflow = previousOverflow;
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [expanded]);

  const resetView = () => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  };

  const isLarge = metrics ? isLargeMermaidDiagram(metrics) : code.length > 8_000 || code.split(/\r?\n/).length > 64;

  const copySource = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_400);
    } catch {
      setCopied(false);
    }
  };

  const beginPan = (event: ReactPointerEvent<HTMLDivElement>) => {
    // 极高或极宽的图在 100% 下也可能超出画布，因此平移不能只在放大后启用。
    if (event.button !== 0) return;
    panRef.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, originX: pan.x, originY: pan.y };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const movePan = (event: ReactPointerEvent<HTMLDivElement>) => {
    const active = panRef.current;
    if (!active || active.pointerId !== event.pointerId) return;
    setPan({ x: active.originX + event.clientX - active.x, y: active.originY + event.clientY - active.y });
  };

  const endPan = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (panRef.current?.pointerId === event.pointerId && event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    panRef.current = null;
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
          {status === "ready" && isLarge ? (
            <button type="button" className="markdown-enhanced-action" title="在大图查看器中打开" aria-label="在大图查看器中打开" onClick={() => setExpanded(true)}>
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
        <>
          <div className={`markdown-mermaid-svg${isLarge ? " markdown-mermaid-collapsed" : ""}`} dangerouslySetInnerHTML={{ __html: svg }} />
          {isLarge ? <button type="button" className="markdown-code-expand" onClick={() => setExpanded(true)}>在查看器中打开完整图表</button> : null}
        </>
      ) : null}
      {expanded && status === "ready" ? (
        <div ref={lightboxRef} className="markdown-mermaid-lightbox" role="dialog" aria-modal="true" aria-label="Mermaid 图表查看器">
          <div className="markdown-mermaid-lightbox-toolbar">
            <span>Mermaid 图表{metrics ? ` · ${Math.round(metrics.width)}×${Math.round(metrics.height)}` : ""}</span>
            <div className="markdown-code-actions">
              <button type="button" className="markdown-enhanced-action" title="缩小" aria-label="缩小" onClick={() => setZoom((value) => Math.max(0.25, Number((value - 0.25).toFixed(2))))}><Minus size={14} /></button>
              <span className="markdown-mermaid-zoom-label">{Math.round(zoom * 100)}%</span>
              <button type="button" className="markdown-enhanced-action" title="放大" aria-label="放大" onClick={() => setZoom((value) => Math.min(4, Number((value + 0.25).toFixed(2))))}><Plus size={14} /></button>
              <button type="button" className="markdown-enhanced-action" title="复位视图" aria-label="复位视图" onClick={resetView}><RotateCcw size={14} /></button>
              <button ref={closeButtonRef} type="button" className="markdown-enhanced-action" title="关闭查看器" aria-label="关闭查看器" onClick={() => { setExpanded(false); resetView(); }}><X size={15} /></button>
            </div>
          </div>
          <div
            className="markdown-mermaid-lightbox-canvas"
            onWheel={(event) => { event.preventDefault(); setZoom((value) => Math.min(4, Math.max(0.25, Number((value + (event.deltaY < 0 ? 0.1 : -0.1)).toFixed(2))))); }}
            onPointerDown={beginPan}
            onPointerMove={movePan}
            onPointerUp={endPan}
            onPointerCancel={endPan}
          >
            <div className="markdown-mermaid-lightbox-svg" style={{ transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})` }} dangerouslySetInnerHTML={{ __html: svg }} />
          </div>
        </div>
      ) : null}
    </div>
  );
}

export default MermaidBlock;
