import { ArrowLeft, Download, Image as ImageIcon, Minus, Plus, RotateCcw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ImageViewerItem } from "./types";
import { loadChatAttachmentImage } from "../api/attachments";
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
  const [zoom, setZoom] = useState(1);
  const [fullSrc, setFullSrc] = useState<string | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [intrinsicSize, setIntrinsicSize] = useState<IntrinsicImageSize | null>(null);
  const decodeBudgetRef = useRef(new DecodedImageBudget(CHAT_VIEWER_DECODE_LIMIT_BYTES));
  const leaseRef = useRef<DecodedImageLease | null>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);
  const lifecycleState = useWorkspaceLifecycle();

  useEffect(() => {
    setZoom(1);
    setFullSrc(null);
    setLoadError(false);
    setIntrinsicSize(null);
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
  const isZoomed = zoom > 1;

  const changeZoom = (nextZoom: number) => {
    const next = Math.max(0.5, Math.min(3, nextZoom));
    const body = bodyRef.current;
    const image = imageRef.current;
    if (!body || !image || image.getBoundingClientRect().width <= 0) {
      setZoom(next);
      return;
    }

    const bodyRect = body.getBoundingClientRect();
    const imageRect = image.getBoundingClientRect();
    const focusX = bodyRect.left + body.clientWidth / 2;
    const focusY = bodyRect.top + body.clientHeight / 2;
    const relativeX = (focusX - imageRect.left) / Math.max(1, imageRect.width);
    const relativeY = (focusY - imageRect.top) / Math.max(1, imageRect.height);

    setZoom(next);
    window.requestAnimationFrame(() => {
      const nextRect = image.getBoundingClientRect();
      const nextBodyRect = body.getBoundingClientRect();
      const nextFocusX = nextRect.left + relativeX * nextRect.width;
      const nextFocusY = nextRect.top + relativeY * nextRect.height;
      body.scrollLeft = Math.max(
        0,
        body.scrollLeft + nextFocusX - (nextBodyRect.left + body.clientWidth / 2),
      );
      body.scrollTop = Math.max(
        0,
        body.scrollTop + nextFocusY - (nextBodyRect.top + body.clientHeight / 2),
      );
    });
  };

  const scaledWidth = intrinsicSize && isZoomed
    ? Math.max(1, Math.round(intrinsicSize.width * zoom))
    : undefined;
  const scaledHeight = intrinsicSize && isZoomed
    ? Math.max(1, Math.round(intrinsicSize.height * zoom))
    : undefined;
  const downloadImage = () => {
    if (!source || !item.downloadFileName) return;
    const anchor = document.createElement("a");
    anchor.href = source;
    anchor.download = item.downloadFileName;
    anchor.rel = "noopener";
    anchor.click();
  };

  return (
    <div className="image-viewer" role="dialog" aria-modal="true" aria-label={t("chat.viewNamedImage", { name: title })}>
      <header className="image-viewer-header">
        <button type="button" className="image-viewer-icon-button" onClick={onClose} aria-label={t("chat.backToChat")} title={t("chat.backToChat")}>
          <ArrowLeft size={18} />
        </button>
        <span className="image-viewer-title"><ImageIcon size={16} />{title}</span>
        <div className="image-viewer-controls">
          <button type="button" className="image-viewer-icon-button" onClick={() => changeZoom(zoom - 0.25)} aria-label={t("chat.zoomOut")} title={t("chat.zoomOut")}>
            <Minus size={15} />
          </button>
          <span className="image-viewer-zoom">{Math.round(zoom * 100)}%</span>
          <button type="button" className="image-viewer-icon-button" onClick={() => changeZoom(zoom + 0.25)} aria-label={t("chat.zoomIn")} title={t("chat.zoomIn")}>
            <Plus size={15} />
          </button>
          <button type="button" className="image-viewer-icon-button" onClick={() => changeZoom(1)} aria-label={t("chat.resetZoom")} title={t("chat.resetZoom")}>
            <RotateCcw size={14} />
          </button>
          {item.downloadFileName ? (
            <button type="button" className="image-viewer-icon-button" onClick={downloadImage} aria-label={t("chat.downloadImage")} title={t("chat.downloadImage")}>
              <Download size={15} />
            </button>
          ) : null}
          <button type="button" className="image-viewer-icon-button" onClick={onClose} aria-label={t("chat.closeImage")} title={t("chat.closeImage")}>
            <X size={17} />
          </button>
        </div>
      </header>
      <div ref={bodyRef} className="image-viewer-body" onClick={onClose}>
        <div className="image-viewer-canvas" data-zoomed={isZoomed ? "true" : "false"}>
          {source ? (
            <img
              ref={imageRef}
              src={source}
              alt={item.alt}
              decoding="async"
              className="image-viewer-image"
              style={{
                width: scaledWidth ? `${scaledWidth}px` : "auto",
                height: scaledHeight ? `${scaledHeight}px` : "auto",
                maxWidth: isZoomed ? "none" : "100%",
                maxHeight: isZoomed ? "none" : "calc(100vh - 112px)",
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
        </div>
      </div>
    </div>
  );
}
