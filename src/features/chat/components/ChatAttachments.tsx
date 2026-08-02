import { ExternalLink, FileText, Image as ImageIcon, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { ChatAttachment, PendingChatAttachment } from "../../../types/attachment";
import { useI18n } from "../../../i18n/I18nProvider";
import { loadChatAttachmentPreview, openChatAttachment } from "../api/attachments";
import { useImageViewer } from "../image-viewer/ImageViewerContext";
import "../styles/chat-attachments.css";

type AttachmentLike = ChatAttachment | PendingChatAttachment;

type ChatAttachmentsProps = {
  attachments: readonly AttachmentLike[];
  conversationId?: string | null;
  variant: "composer" | "message";
  onRemove?: (attachment: AttachmentLike) => void;
};

function formatFileSize(sizeBytes: number) {
  if (sizeBytes < 1024) return `${sizeBytes} B`;
  if (sizeBytes < 1024 * 1024) return `${(sizeBytes / 1024).toFixed(1)} KB`;
  return `${(sizeBytes / 1024 / 1024).toFixed(1)} MB`;
}

function AttachmentImage({
  attachment,
  conversationId,
  onPreview,
}: {
  attachment: AttachmentLike;
  conversationId?: string | null;
  onPreview?: (src: string) => void;
}) {
  const { t } = useI18n();
  const [preview, setPreview] = useState("");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setPreview("");
    setFailed(false);
    const previewLoad = loadChatAttachmentPreview(
      attachment.path,
      conversationId,
      attachment.previewPath,
    );
    void previewLoad.promise
      .then((dataUrl) => {
        if (!cancelled) setPreview(dataUrl);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
      previewLoad.release();
    };
  }, [attachment.path, attachment.previewPath, conversationId]);

  if (!preview || failed) {
    return (
      <span className="chat-attachment-image-fallback" aria-hidden="true">
        <ImageIcon size={19} />
      </span>
    );
  }
  return (
    <button
      type="button"
      className="chat-attachment-image-button"
      onClick={() => onPreview?.(preview)}
      title={t("chat.previewImage")}
      aria-label={t("chat.previewNamedImage", { name: attachment.name })}
    >
      <img className="chat-attachment-image" src={preview} alt={attachment.name} />
    </button>
  );
}

export function ChatAttachments({
  attachments,
  conversationId = null,
  variant,
  onRemove,
}: ChatAttachmentsProps) {
  const { t } = useI18n();
  const { openImage } = useImageViewer();
  if (attachments.length === 0) return null;

  return (
    <div className={`chat-attachments chat-attachments-${variant}`}>
      {attachments.map((attachment) => (
        <div
          className={`chat-attachment chat-attachment-${attachment.kind}`}
          key={attachment.id}
          title={`${attachment.name} (${formatFileSize(attachment.sizeBytes)})`}
        >
          {attachment.kind === "image" ? (
            <AttachmentImage
              attachment={attachment}
              conversationId={conversationId}
              onPreview={(src) => openImage({
                src,
                alt: attachment.name,
                title: attachment.name,
                conversationId,
                attachmentPath: conversationId ? attachment.path : null,
              })}
            />
          ) : (
            <span className="chat-attachment-file-icon" aria-hidden="true">
              <FileText size={18} />
            </span>
          )}
          <span className="chat-attachment-copy">
            <strong>{attachment.name}</strong>
            <small>{formatFileSize(attachment.sizeBytes)}</small>
          </span>
          {variant === "message" && conversationId && attachment.kind !== "image" ? (
            <button
              className="chat-attachment-open"
              type="button"
              title={t("chat.openAttachmentExternal")}
              aria-label={`打开附件 ${attachment.name}`}
              onClick={() => void openChatAttachment(conversationId, attachment.path)}
            >
              <ExternalLink size={14} />
            </button>
          ) : null}
          {onRemove ? (
            <button
              className="chat-attachment-remove"
              type="button"
              title="移除附件"
              aria-label={`移除附件 ${attachment.name}`}
              onClick={() => onRemove(attachment)}
            >
              <X size={14} />
            </button>
          ) : null}
        </div>
      ))}
    </div>
  );
}
