import { useEffect, useMemo, useRef, useState, type UIEvent as ReactUIEvent } from "react";
import { Check, Code2, Copy, Eye, LoaderCircle, Maximize2, Minimize2, RotateCcw, X } from "lucide-react";
import { useElementVisibility } from "../hooks/useElementVisibility";
import {
  getMermaidPreviewViewMode,
  getMermaidScrollLayout,
  getMermaidViewerViewport,
  isLargeMermaidDiagram,
} from "../utils/mermaidLayout";
import { renderMermaid } from "../utils/mermaidRuntime";
import { sanitizeMermaidSvg, mermaidThemeConfig, type MermaidSvgMetrics } from "../utils/mermaidSecurity";
import { MARKDOWN_RENDER_LIMITS } from "../utils/renderLimits";
import {
  renderMermaidSvgInShadowHost,
  updateMermaidSvgViewport,
  type MermaidShadowViewport,
} from "../utils/mermaidShadow";
import "../styles/enhanced-markdown.css";

type MermaidBlockProps = {
  code: string;
  streaming?: boolean;
};

type Status = "source" | "loading" | "ready" | "error";

let renderSequence = 0;
const MERMAID_RENDER_DEBOUNCE_MS = 120;

function MermaidSvgHost({ svg, className, viewport }: {
  svg: string;
  className: string;
  viewport?: MermaidShadowViewport;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const hasViewport = viewport !== undefined;

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !svg) return;
    host.toggleAttribute("data-mermaid-viewport", hasViewport);
    try {
      renderMermaidSvgInShadowHost(svg, host, viewport);
    } catch (reason) {
      console.error("Mermaid SVG 挂载失败", reason);
    }
  }, [svg, hasViewport]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !viewport) return;
    host.setAttribute("data-mermaid-viewport", "true");
    updateMermaidSvgViewport(host, viewport);
  }, [viewport?.x, viewport?.y, viewport?.width, viewport?.height]);

  return <div ref={hostRef} className={className} data-testid="mermaid-shadow-host" />;
}

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
  const [previewWidth, setPreviewWidth] = useState(900);
  const [canvasSize, setCanvasSize] = useState({ width: 1_200, height: 700 });
  const [scrollPosition, setScrollPosition] = useState({ left: 0, top: 0 });
  const surfaceRef = useRef<HTMLDivElement>(null);
  const scrollportRef = useRef<HTMLDivElement>(null);
  const previewResizeFrameRef = useRef<number | null>(null);
  const canvasResizeFrameRef = useRef<number | null>(null);
  const scrollFrameRef = useRef<number | null>(null);
  const lastPreviewWidthRef = useRef(900);

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
    const host = ref.current;
    if (!host) return;
    const scheduleMeasurement = () => {
      if (previewResizeFrameRef.current !== null) return;
      previewResizeFrameRef.current = window.requestAnimationFrame(() => {
        previewResizeFrameRef.current = null;
        const width = Math.max(280, Math.round(host.getBoundingClientRect().width - 32));
        if (Math.abs(lastPreviewWidthRef.current - width) < 1) return;
        lastPreviewWidthRef.current = width;
        setPreviewWidth(width);
      });
    };
    scheduleMeasurement();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(scheduleMeasurement);
    observer.observe(host);
    return () => {
      observer.disconnect();
      if (previewResizeFrameRef.current !== null) window.cancelAnimationFrame(previewResizeFrameRef.current);
      previewResizeFrameRef.current = null;
    };
  }, [ref]);

  useEffect(() => {
    let cancelled = false;
    if (streaming || showSource) return () => { cancelled = true; };
    if (!visible) {
      setSvg((current) => current ? "" : current);
      setStatus((current) => current === "ready" ? "source" : current);
      return () => { cancelled = true; };
    }
    if (code.length > MARKDOWN_RENDER_LIMITS.maxMermaidChars) {
      setStatus("error");
      setError("图表内容过长，已保留源代码。");
      return () => { cancelled = true; };
    }

    setStatus("loading");
    setError("");
    setMetrics(null);
    const host = ref.current;
    if (!host) return () => { cancelled = true; };
    const currentId = `mnemora-mermaid-${++renderSequence}`;
    const timer = window.setTimeout(() => {
      if (cancelled) return;
      const width = Math.max(280, host.getBoundingClientRect().width - 32);
      void renderMermaid(code, currentId, mermaidThemeConfig(host), width).then((result) => {
        if (cancelled) return;
        const sanitized = sanitizeMermaidSvg(result.svg);
        setSvg(sanitized.svg);
        setMetrics(sanitized.metrics);
        setScrollPosition({ left: 0, top: 0 });
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
    if (!expanded) return;
    const surface = surfaceRef.current;
    if (!surface) return;
    let lastWidth = 0;
    let lastHeight = 0;
    const scheduleMeasurement = () => {
      if (canvasResizeFrameRef.current !== null) return;
      canvasResizeFrameRef.current = window.requestAnimationFrame(() => {
        canvasResizeFrameRef.current = null;
        const bounds = surface.getBoundingClientRect();
        const width = Math.max(1, Math.round(bounds.width));
        const height = Math.max(1, Math.round(bounds.height));
        if (width === lastWidth && height === lastHeight) return;
        lastWidth = width;
        lastHeight = height;
        setCanvasSize({ width, height });
      });
    };
    scheduleMeasurement();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleMeasurement);
    observer?.observe(surface);
    return () => {
      observer?.disconnect();
      if (canvasResizeFrameRef.current !== null) window.cancelAnimationFrame(canvasResizeFrameRef.current);
      canvasResizeFrameRef.current = null;
    };
  }, [expanded]);

  useEffect(() => {
    if (expanded && metrics?.viewerSafe !== true) setExpanded(false);
  }, [expanded, metrics?.viewerSafe]);

  useEffect(() => () => {
    if (scrollFrameRef.current !== null) window.cancelAnimationFrame(scrollFrameRef.current);
  }, []);

  const isLarge = metrics ? isLargeMermaidDiagram(metrics, previewWidth) : code.length > 8_000 || code.split(/\r?\n/).length > 64;
  const previewHeight = Math.round(Math.min(560, Math.max(280, previewWidth * 0.58)));
  const previewViewport = metrics && isLarge
    ? getMermaidViewerViewport(
      metrics,
      { width: previewWidth, height: previewHeight },
      getMermaidPreviewViewMode(metrics, { width: previewWidth, height: previewHeight }),
    )
    : undefined;
  const scrollLayout = useMemo(() => metrics
    ? getMermaidScrollLayout(metrics, canvasSize, scrollPosition)
    : null, [canvasSize, metrics, scrollPosition]);
  const activeViewport = expanded ? scrollLayout?.viewport : previewViewport;
  const viewerSafe = metrics?.viewerSafe === true;

  const copySource = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1_400);
    } catch {
      setCopied(false);
    }
  };

  const toggleExpanded = () => {
    if (!viewerSafe) return;
    setExpanded((current) => {
      if (!current) {
        setScrollPosition({ left: 0, top: 0 });
        window.requestAnimationFrame(() => scrollportRef.current?.focus({ preventScroll: true }));
      }
      return !current;
    });
  };

  const handleScroll = (event: ReactUIEvent<HTMLDivElement>) => {
    const target = event.currentTarget;
    if (scrollFrameRef.current !== null) return;
    scrollFrameRef.current = window.requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      setScrollPosition({ left: target.scrollLeft, top: target.scrollTop });
    });
  };

  const renderButton = status === "ready" && !showSource ? (
    <button type="button" className="markdown-enhanced-action" title="查看 Mermaid 源代码" aria-label="查看 Mermaid 源代码" onClick={() => { setExpanded(false); setShowSource(true); }}>
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
          {status === "ready" && viewerSafe ? (
            <button
              type="button"
              className="markdown-enhanced-action"
              title={expanded ? "收起 Mermaid 图表" : "展开 Mermaid 图表"}
              aria-label={expanded ? "收起 Mermaid 图表" : "展开 Mermaid 图表"}
              aria-expanded={expanded}
              onClick={toggleExpanded}
            >
              {expanded ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
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
          <div
            ref={surfaceRef}
            className={`markdown-mermaid-surface${expanded ? " markdown-mermaid-surface-open" : isLarge ? " markdown-mermaid-surface-large" : ""}`}
            style={!expanded && isLarge ? { height: `${previewHeight}px` } : undefined}
          >
            <MermaidSvgHost
              className={`markdown-mermaid-svg${activeViewport ? " markdown-mermaid-viewport" : ""}`}
              viewport={activeViewport}
              svg={svg}
            />
            {expanded && scrollLayout ? (
              <div
                ref={scrollportRef}
                className="markdown-mermaid-scrollport"
                tabIndex={0}
                aria-label="Mermaid 图表，可滚动查看完整内容。按 Escape 收起。"
                onKeyDown={(event) => { if (event.key === "Escape") toggleExpanded(); }}
                onScroll={handleScroll}
              >
                <div className="markdown-mermaid-scroll-space" style={{ width: scrollLayout.contentWidth, height: scrollLayout.contentHeight }} />
              </div>
            ) : null}
          </div>
          {isLarge && !viewerSafe ? <div className="markdown-mermaid-budget-warning" role="status">图表过于复杂，已保留有界预览；为避免页面卡死，不提供交互展开。建议复制源代码后按主题拆分图表。</div> : null}
        </>
      ) : null}
    </div>
  );
}

export default MermaidBlock;
