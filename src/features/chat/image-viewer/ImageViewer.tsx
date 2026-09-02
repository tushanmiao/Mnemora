import { ArrowLeft, Copy, Download, Image as ImageIcon, LoaderCircle, Maximize, Minus, Plus, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import type { ImageViewerItem } from "./types";
import { loadChatAttachmentImage } from "../api/attachments";
import { exportDiagramFile } from "../api/diagrams";
import {
  base64ToBytes,
  decodeSvgDataUrl,
  diagramSaveOptions,
  textToBase64,
  type DiagramExportFormat,
} from "../markdown/utils/diagramExport";
import { rasterizeSvgToPng } from "./rasterize";
import { clampZoom, MAX_ZOOM, resolveWheelGesture } from "./wheelGesture";
import { useI18n } from "../../../i18n/I18nProvider";
import {
  CHAT_VIEWER_DECODE_LIMIT_BYTES,
  DecodedImageBudget,
  estimateDecodedImageBytes,
  type DecodedImageLease,
} from "../runtime/imageDecodeBudget";
import { useWorkspaceLifecycle } from "../../../runtime/resources/useWorkspaceLifecycle";
import "./image-viewer.css";

type ImageViewerProps = {
  item: ImageViewerItem;
  onClose: () => void;
};

type IntrinsicImageSize = {
  width: number;
  height: number;
};

export function ImageViewer({ item, onClose }: ImageViewerProps) {
  const { t } = useI18n();
  // null = 还没手动缩放，跟随「适应窗口」。这样改窗口大小时图像会自己贴合，
  // 一旦用户动过缩放就锁定他的选择。
  const [zoom, setZoom] = useState<number | null>(null);
  const [viewportSize, setViewportSize] = useState<{ width: number; height: number } | null>(null);
  const [fullSrc, setFullSrc] = useState<string | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [intrinsicSize, setIntrinsicSize] = useState<IntrinsicImageSize | null>(null);
  const [exporting, setExporting] = useState(false);
  const [copying, setCopying] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  // 成功后要告诉用户拿到的是 PNG 还是 SVG：降级发生在幕后，不说等于骗人。
  const [exportedFormat, setExportedFormat] = useState<DiagramExportFormat | null>(null);
  const [copiedFormat, setCopiedFormat] = useState<DiagramExportFormat | null>(null);
  const decodeBudgetRef = useRef(new DecodedImageBudget(CHAT_VIEWER_DECODE_LIMIT_BYTES));
  const leaseRef = useRef<DecodedImageLease | null>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);
  const lifecycleState = useWorkspaceLifecycle();

  useEffect(() => {
    setZoom(null);
    setFullSrc(null);
    setLoadError(false);
    setIntrinsicSize(null);
    setExportError(null);
    setExportedFormat(null);
    setCopiedFormat(null);
    leaseRef.current?.release();
    leaseRef.current = null;
    if (lifecycleState !== "active" || !item.conversationId || !item.attachmentPath) return;
    const load = loadChatAttachmentImage(item.conversationId, item.attachmentPath);
    let cancelled = false;
    void load.promise.then((source) => {
      if (cancelled) return;
      const lease = decodeBudgetRef.current.reserve({
        owner: `image-viewer:${item.conversationId}:${item.attachmentPath}`,
        estimatedBytes: estimateDecodedImageBytes(source.width, source.height),
        onEvict: () => {
          if (!cancelled) {
            setFullSrc(null);
            setLoadError(true);
          }
        },
      });
      if (!lease) {
        setLoadError(true);
        return;
      }
      leaseRef.current = lease;
      setFullSrc(source.src);
    }).catch(() => {
      if (!cancelled) setLoadError(true);
    });
    return () => {
      cancelled = true;
      load.release();
      leaseRef.current?.release();
      leaseRef.current = null;
      setFullSrc(null);
    };
  }, [item.attachmentPath, item.conversationId, item.src, lifecycleState]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const title = item.title || item.alt || t("chat.image");
  const source = lifecycleState === "active" ? fullSrc ?? item.src : "";

  /**
   * 观测可视区尺寸，「适应窗口」要按它算。
   *
   * 用 ResizeObserver 而不是 window.resize：面板宽度还会被侧栏折叠影响，
   * 那种变化不产生 window.resize 事件。
   *
   * 必须取 **content box**：`clientWidth` 含 padding，而 `.image-viewer-body`
   * 有 22px 内边距，用它算出的「适应」会比可用空间宽 44px —— 点完重置照样有
   * 滚动条。`contentBoxSize` 正好是去掉 padding 的尺寸。
   */
  useEffect(() => {
    const body = bodyRef.current;
    if (!body) return;
    const fallbackSize = () => {
      const styles = getComputedStyle(body);
      const horizontal = (Number.parseFloat(styles.paddingLeft) || 0)
        + (Number.parseFloat(styles.paddingRight) || 0);
      const vertical = (Number.parseFloat(styles.paddingTop) || 0)
        + (Number.parseFloat(styles.paddingBottom) || 0);
      return {
        width: Math.max(0, body.clientWidth - horizontal),
        height: Math.max(0, body.clientHeight - vertical),
      };
    };
    setViewportSize(fallbackSize());
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const content = entries[0]?.contentBoxSize?.[0];
      setViewportSize(content
        ? { width: content.inlineSize, height: content.blockSize }
        : fallbackSize());
    });
    observer.observe(body);
    return () => observer.disconnect();
  }, [source]);

  /**
   * 「适应窗口」对应的缩放值。
   *
   * 原来把它和 100% 混为一谈：`zoom <= 1` 走 `maxWidth: 100%` 自适应，于是 50%、
   * 75%、100% 渲染出来一模一样（缩小按钮等于没接线），跨过 1.0 又会尺寸暴跳。
   * 把「适应」算成一个具体倍数后，所有档位都是同一套 `固有尺寸 × zoom`。
   */
  const fitZoom = useMemo(() => {
    const viewport = viewportSize;
    if (!intrinsicSize || !viewport || intrinsicSize.width <= 0 || intrinsicSize.height <= 0) {
      return 1;
    }
    const scale = Math.min(
      viewport.width / intrinsicSize.width,
      viewport.height / intrinsicSize.height,
    );
    // 小图不放大：适应窗口的语义是「别让我横向拖」，不是「铺满」。
    // 不夹下限：超宽图的适应值可能远低于 50%，夹了就还会有滚动条。
    // 上限仍然要夹，避免小图被放到 300% 以上。
    return Math.min(MAX_ZOOM, Math.min(1, scale));
  }, [intrinsicSize, viewportSize]);

  // 未手动缩放前跟着窗口走，改过之后就锁住用户的选择。
  const effectiveZoom = zoom ?? fitZoom;
  const atFit = Math.abs(effectiveZoom - fitZoom) < 0.001;

  /**
   * 改变缩放，并让 `focus` 指定的那个点在缩放前后停在屏幕同一位置。
   *
   * `focus` 缺省时用视口中心——按钮缩放该以中心为锚，而 Ctrl+滚轮必须以光标为锚，
   * 否则用户盯着的细节会在缩放瞬间跑掉，这正是「找不到中心在哪里」的来源。
   *
   * 锚点算法只在「图像两个方向都能滚」时才严格成立。图像比容器小的那一维是被
   * flex 居中的，没有滚动量可用来补偿，此时按容器中心处理——强行按光标算会得出
   * 一个负的 scrollTop，被浏览器截断成 0，反而制造出更大的跳动。
   */
  const changeZoom = useCallback((nextZoom: number, focus?: { x: number; y: number }) => {
    // 下限放到 fitZoom：手动缩到「比适应窗口更小」没有意义，但适应值本身必须可达。
    const next = clampZoom(nextZoom, fitZoom);
    const body = bodyRef.current;
    const image = imageRef.current;
    if (!body || !image || image.getBoundingClientRect().width <= 0) {
      setZoom(next);
      return;
    }

    const bodyRect = body.getBoundingClientRect();
    const imageRect = image.getBoundingClientRect();
    // 只有溢出的那一维能靠滚动补偿；另一维交给居中，锚点取容器中心。
    const scrollableX = image.offsetWidth > body.clientWidth + 1;
    const scrollableY = image.offsetHeight > body.clientHeight + 1;
    const centerX = bodyRect.left + body.clientWidth / 2;
    const centerY = bodyRect.top + body.clientHeight / 2;
    const focusX = scrollableX ? focus?.x ?? centerX : centerX;
    const focusY = scrollableY ? focus?.y ?? centerY : centerY;
    // 锚点在图像内的相对位置（0~1）。缩放后仍要落在同一相对位置上。
    const relativeX = (focusX - imageRect.left) / Math.max(1, imageRect.width);
    const relativeY = (focusY - imageRect.top) / Math.max(1, imageRect.height);
    // 锚点相对视口左上角的偏移。滚动量按这个偏移回推，锚点才不会漂移。
    const viewportOffsetX = focusX - bodyRect.left;
    const viewportOffsetY = focusY - bodyRect.top;

    setZoom(next);
    window.requestAnimationFrame(() => {
      const nextRect = image.getBoundingClientRect();
      const nextBodyRect = body.getBoundingClientRect();
      if (image.offsetWidth > body.clientWidth + 1) {
        const nextFocusX = nextRect.left + relativeX * nextRect.width;
        body.scrollLeft = Math.max(
          0,
          body.scrollLeft + nextFocusX - (nextBodyRect.left + viewportOffsetX),
        );
      }
      if (image.offsetHeight > body.clientHeight + 1) {
        const nextFocusY = nextRect.top + relativeY * nextRect.height;
        body.scrollTop = Math.max(
          0,
          body.scrollTop + nextFocusY - (nextBodyRect.top + viewportOffsetY),
        );
      }
    });
  }, [fitZoom]);

  const scaledWidth = intrinsicSize
    ? Math.max(1, Math.round(intrinsicSize.width * effectiveZoom))
    : undefined;
  const scaledHeight = intrinsicSize
    ? Math.max(1, Math.round(intrinsicSize.height * effectiveZoom))
    : undefined;
  // 任一维超出视口就要开滚动、并让 canvas 从左上角排布，否则居中会裁掉内容。
  const overflowing = Boolean(
    viewportSize && scaledWidth && scaledHeight
    && (scaledWidth > viewportSize.width + 1 || scaledHeight > viewportSize.height + 1),
  );
  /**
   * 导出当前图像。
   *
   * 不用 `<a download>`：WebView2 不认这个属性，点了等于什么都没发生（这就是原来
   * 的 bug）。改成「保存对话框 + Rust 写盘」，用户能选位置，也能拿到失败原因。
   *
   * 矢量源优先导出 PNG——SVG 换台机器少个字体就变形。栅格化失败（`foreignObject`
   * 画不出、画布超限）时降级写 SVG 文本，而不是让这个按钮再次静默失效。
   */
  const exportImage = useCallback(async () => {
    if (!source || !item.downloadFileName || exporting) return;
    const svgText = decodeSvgDataUrl(source);
    if (!svgText) {
      // 位图附件已经在磁盘上，用不着经查看器再存一份。
      setExportError(t("chat.downloadImageUnsupported"));
      return;
    }

    setExporting(true);
    setExportError(null);
    try {
      const rect = imageRef.current?.getBoundingClientRect();
      const raster = await rasterizeSvgToPng(source, rect?.width ?? 0, rect?.height ?? 0);
      const format: DiagramExportFormat = raster ? "png" : "svg";
      const base64 = raster ? raster.base64 : textToBase64(svgText);

      const options = diagramSaveOptions(format, item.downloadFileName);
      const path = await save({ title: t("chat.downloadImage"), ...options });
      if (typeof path !== "string") return;

      await exportDiagramFile(path, base64);
      setExportedFormat(format);
    } catch (error) {
      setExportError(error instanceof Error ? error.message : String(error));
    } finally {
      setExporting(false);
    }
  }, [exporting, item.downloadFileName, source, t]);

  /**
   * 复制图像到剪贴板。
   *
   * PNG 优先：只有位图才能粘进 Word、微信这类应用。栅格化或 `ClipboardItem`
   * 不可用时退回复制 SVG 文本——粘贴目标少，但至少不是「点了没反应」。
   */
  const copyImage = useCallback(async () => {
    if (!source || copying) return;
    const svgText = decodeSvgDataUrl(source);
    if (!svgText) {
      setExportError(t("chat.copyImageUnsupported"));
      return;
    }

    setCopying(true);
    setExportError(null);
    setCopiedFormat(null);
    try {
      const rect = imageRef.current?.getBoundingClientRect();
      const raster = typeof ClipboardItem === "function"
        ? await rasterizeSvgToPng(source, rect?.width ?? 0, rect?.height ?? 0)
        : null;
      if (raster) {
        const blob = new Blob([base64ToBytes(raster.base64)], { type: "image/png" });
        await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
        setCopiedFormat("png");
      } else {
        await navigator.clipboard.writeText(svgText);
        setCopiedFormat("svg");
      }
      window.setTimeout(() => setCopiedFormat(null), 2_000);
    } catch (error) {
      setExportError(error instanceof Error ? error.message : String(error));
    } finally {
      setCopying(false);
    }
  }, [copying, source, t]);

  /**
   * 裸滚轮平移、Ctrl+滚轮缩放。
   *
   * 只在真正处理了手势时 `preventDefault`：贴在缩放边界上还拦着，会让用户觉得
   * 整个面板卡死。判定本身在 `resolveWheelGesture` 里，这里只负责施加结果。
   */
  const handleWheel = useCallback((event: React.WheelEvent<HTMLDivElement>) => {
    const body = bodyRef.current;
    if (!body) return;
    const outcome = resolveWheelGesture(event, effectiveZoom, fitZoom);
    if (outcome.kind === "ignore") return;
    event.preventDefault();
    if (outcome.kind === "zoom") {
      changeZoom(outcome.zoom, { x: event.clientX, y: event.clientY });
      return;
    }
    body.scrollLeft += outcome.dx;
    body.scrollTop += outcome.dy;
  }, [changeZoom, effectiveZoom, fitZoom]);

  return (
    <div className="image-viewer" role="dialog" aria-modal="true" aria-label={t("chat.viewNamedImage", { name: title })}>
      <header className="image-viewer-header">
        <button type="button" className="image-viewer-icon-button" onClick={onClose} aria-label={t("chat.backToChat")} title={t("chat.backToChat")}>
          <ArrowLeft size={18} />
        </button>
        <span className="image-viewer-title"><ImageIcon size={16} />{title}</span>
        <div className="image-viewer-controls">
          <button type="button" className="image-viewer-icon-button" onClick={() => changeZoom(effectiveZoom - 0.25)} aria-label={t("chat.zoomOut")} title={t("chat.zoomOut")}>
            <Minus size={15} />
          </button>
          <span className="image-viewer-zoom">{Math.round(effectiveZoom * 100)}%</span>
          <button type="button" className="image-viewer-icon-button" onClick={() => changeZoom(effectiveZoom + 0.25)} aria-label={t("chat.zoomIn")} title={t("chat.zoomIn")}>
            <Plus size={15} />
          </button>
          {/* 用 Maximize 而不是 RotateCcw：后者在 MermaidBlock 里是「重试渲染」，
              一个图标两种语义，用户认不出这是重置。回到「适应窗口」而不是 100%——
              重置的诉求是「让我重新看到整张图」。 */}
          <button
            type="button"
            className="image-viewer-icon-button"
            onClick={() => setZoom(null)}
            disabled={atFit}
            aria-label={t("chat.resetZoom")}
            title={t("chat.resetZoom")}
          >
            <Maximize size={14} />
          </button>
          <button
            type="button"
            className="image-viewer-icon-button"
            onClick={() => void copyImage()}
            disabled={copying}
            aria-label={t("chat.copyImage")}
            title={copiedFormat ? t("chat.copyImageDone") : t("chat.copyImage")}
          >
            {copying ? <LoaderCircle size={15} className="image-viewer-spin" /> : <Copy size={15} />}
          </button>
          {item.downloadFileName ? (
            <button
              type="button"
              className="image-viewer-icon-button"
              onClick={() => void exportImage()}
              disabled={exporting}
              aria-label={t("chat.downloadImage")}
              title={t("chat.downloadImage")}
            >
              {exporting ? <LoaderCircle size={15} className="image-viewer-spin" /> : <Download size={15} />}
            </button>
          ) : null}
          <button type="button" className="image-viewer-icon-button" onClick={onClose} aria-label={t("chat.closeImage")} title={t("chat.closeImage")}>
            <X size={17} />
          </button>
        </div>
      </header>
      <div ref={bodyRef} className="image-viewer-body" onClick={onClose} onWheel={handleWheel}>
        {/* data-zoomed 只表示「有一维溢出、需要靠滚动看全」，不再等同于 zoom>1 */}
        <div className="image-viewer-canvas" data-zoomed={overflowing ? "true" : "false"}>
          {source ? (
            <img
              ref={imageRef}
              src={source}
              alt={item.alt}
              decoding="async"
              className="image-viewer-image"
              style={{
                // 所有档位统一走「固有尺寸 × zoom」，包括适应窗口那一档。
                // 原来 zoom<=1 走 maxWidth:100% 自适应，导致 50/75/100% 渲染相同。
                width: scaledWidth ? `${scaledWidth}px` : "auto",
                height: scaledHeight ? `${scaledHeight}px` : "auto",
                maxWidth: "none",
                maxHeight: "none",
              }}
              onLoad={(event) => {
                const image = event.currentTarget;
                if (image.naturalWidth > 0 && image.naturalHeight > 0) {
                  setIntrinsicSize({ width: image.naturalWidth, height: image.naturalHeight });
                }
              }}
              onClick={(event) => event.stopPropagation()}
            />
          ) : null}
          {loadError ? <span className="image-viewer-notice">{t("chat.originalImageFailed")}</span> : null}
          {exportError ? (
            <span className="image-viewer-notice" role="alert">{exportError}</span>
          ) : exportedFormat === "svg" ? (
            <span className="image-viewer-notice" role="status">{t("chat.downloadImageSvgFallback")}</span>
          ) : copiedFormat === "svg" ? (
            <span className="image-viewer-notice" role="status">{t("chat.copyImageSvgFallback")}</span>
          ) : copiedFormat === "png" ? (
            <span className="image-viewer-notice" role="status">{t("chat.copyImageDone")}</span>
          ) : null}
        </div>
      </div>
    </div>
  );
}
