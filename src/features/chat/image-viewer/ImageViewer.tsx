import { ArrowLeft, Image as ImageIcon, Minus, Plus, RotateCcw, X } from "lucide-react";
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

export function ImageViewer({ item, onClose }: ImageViewerProps) {
  const { t } = useI18n();
  const [zoom, setZoom] = useState(1);
  const [fullSrc, setFullSrc] = useState<string | null>(null);
  const [loadError, setLoadError] = useState(false);
  const decodeBudgetRef = useRef(new DecodedImageBudget(CHAT_VIEWER_DECODE_LIMIT_BYTES));
  const leaseRef = useRef<DecodedImageLease | null>(null);
  const lifecycleState = useWorkspaceLifecycle();

  useEffect(() => {
    setZoom(1);
    setFullSrc(null);
    setLoadError(false);
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

  return (
    <div className="image-viewer" role="dialog" aria-modal="true" aria-label={t("chat.viewNamedImage", { name: title })}>
      <header className="image-viewer-header">
        <button type="button" className="image-viewer-icon-button" onClick={onClose} aria-label={t("chat.backToChat")} title={t("chat.backToChat")}>
          <ArrowLeft size={18} />
        </button>
        <span className="image-viewer-title"><ImageIcon size={16} />{title}</span>
        <div className="image-viewer-controls">
          <button type="button" className="image-viewer-icon-button" onClick={() => setZoom((value) => Math.max(0.5, value - 0.25))} aria-label={t("chat.zoomOut")} title={t("chat.zoomOut")}>
            <Minus size={15} />
          </button>
          <span className="image-viewer-zoom">{Math.round(zoom * 100)}%</span>
          <button type="button" className="image-viewer-icon-button" onClick={() => setZoom((value) => Math.min(3, value + 0.25))} aria-label={t("chat.zoomIn")} title={t("chat.zoomIn")}>
            <Plus size={15} />
          </button>
          <button type="button" className="image-viewer-icon-button" onClick={() => setZoom(1)} aria-label={t("chat.resetZoom")} title={t("chat.resetZoom")}>
            <RotateCcw size={14} />
          </button>
          <button type="button" className="image-viewer-icon-button" onClick={onClose} aria-label={t("chat.closeImage")} title={t("chat.closeImage")}>
            <X size={17} />
          </button>
        </div>
      </header>
      <div className="image-viewer-body" onClick={onClose}>
        <div className="image-viewer-canvas">
          {source ? (
            <img
              src={source}
              alt={item.alt}
              decoding="async"
              className="image-viewer-image"
              style={{
                width: zoom <= 1 ? "auto" : `${zoom * 100}%`,
                maxWidth: zoom <= 1 ? "100%" : "none",
                maxHeight: zoom <= 1 ? "calc(100vh - 112px)" : "none",
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
