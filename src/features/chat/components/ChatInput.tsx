import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  ArrowUp,
  BookOpenText,
  LoaderCircle,
  Paperclip,
  SlidersHorizontal,
  Square,
} from "lucide-react";
import type { ChatAttachment, PendingChatAttachment } from "../../../types/attachment";
import {
  cancelChatAttachmentTask,
  discardImportedChatAttachments,
  discardStagedChatAttachment,
  importChatAttachments,
  inspectChatAttachments,
  savePastedChatAttachment,
} from "../api/attachments";
import type { ContextUsageEstimate } from "../utils/contextUsage";
import { ChatAttachments } from "./ChatAttachments";
import { ContextUsageIndicator } from "./ContextUsageIndicator";
import "../styles/chat-input.css";

const MAX_ATTACHMENTS = 8;

type ChatInputProps = {
  conversationId: string | null;
  disabled?: boolean;
  busy?: boolean;
  stopDisabled?: boolean;
  placeholder?: string;
  contextUsage: ContextUsageEstimate;
  contextWindowTokens: number | null;
  contextMessageCount: number;
  contextCompressionCount?: number;
  contextDisabled?: boolean;
  onSend: (content: string, attachments?: ChatAttachment[]) => void;
  onStop?: () => void;
};

function fileToBase64(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("读取剪贴板附件失败。"));
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("读取剪贴板附件失败。"));
        return;
      }
      resolve(result.slice(result.indexOf(",") + 1));
    };
    reader.readAsDataURL(file);
  });
}

function attachmentName(file: File, index: number) {
  if (file.name.trim()) return file.name;
  if (!file.type.startsWith("image/")) return `clipboard-${Date.now()}-${index + 1}.bin`;
  const extension = file.type === "image/jpeg"
    ? "jpg"
    : file.type === "image/webp"
      ? "webp"
      : file.type === "image/gif"
        ? "gif"
        : "png";
  return `clipboard-${Date.now()}-${index + 1}.${extension}`;
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : fallback;
}

export function ChatInput({
  conversationId,
  disabled = false,
  busy = false,
  stopDisabled = false,
  placeholder = "向 Mnemora 提问...",
  contextUsage,
  contextWindowTokens,
  contextMessageCount,
  contextCompressionCount = 0,
  contextDisabled = false,
  onSend,
  onStop,
}: ChatInputProps) {
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<PendingChatAttachment[]>([]);
  const [attachmentError, setAttachmentError] = useState("");
  const [preparingAttachments, setPreparingAttachments] = useState(false);
  const preparingAttachmentsRef = useRef(false);
  const attachmentsRef = useRef(attachments);
  const attachmentSessionRef = useRef(0);
  const activeAttachmentTaskRef = useRef<string | null>(null);
  const previousConversationIdRef = useRef(conversationId);
  attachmentsRef.current = attachments;
  const inputDisabled = disabled || busy || preparingAttachments;
  const canSend = !inputDisabled && (draft.trim().length > 0 || attachments.length > 0);

  const discardPendingAttachments = (items: readonly PendingChatAttachment[]) => {
    for (const attachment of items) {
      void discardStagedChatAttachment(attachment.path);
    }
  };

  useEffect(() => {
    if (previousConversationIdRef.current === conversationId) return;
    previousConversationIdRef.current = conversationId;
    attachmentSessionRef.current += 1;
    const activeTask = activeAttachmentTaskRef.current;
    activeAttachmentTaskRef.current = null;
    if (activeTask) void cancelChatAttachmentTask(activeTask);
    const pending = attachmentsRef.current;
    attachmentsRef.current = [];
    setAttachments([]);
    setAttachmentError("");
    discardPendingAttachments(pending);
  }, [conversationId]);

  useEffect(() => () => {
    attachmentSessionRef.current += 1;
    const activeTask = activeAttachmentTaskRef.current;
    activeAttachmentTaskRef.current = null;
    if (activeTask) void cancelChatAttachmentTask(activeTask);
    discardPendingAttachments(attachmentsRef.current);
  }, []);

  const addAttachments = (incoming: PendingChatAttachment[]) => {
    const current = attachmentsRef.current;
    const existing = new Set(current.map((attachment) => attachment.path));
    const duplicates: PendingChatAttachment[] = [];
    const unique = incoming.filter((attachment) => {
      if (existing.has(attachment.path)) {
        duplicates.push(attachment);
        return false;
      }
      existing.add(attachment.path);
      return true;
    });
    discardPendingAttachments(duplicates);
    const available = Math.max(0, MAX_ATTACHMENTS - current.length);
    if (unique.length > available) {
      setAttachmentError(`每条消息最多添加 ${MAX_ATTACHMENTS} 个附件。`);
      for (const attachment of unique.slice(available)) {
        void discardStagedChatAttachment(attachment.path);
      }
    } else {
      setAttachmentError("");
    }
    const next = [...current, ...unique.slice(0, available)];
    attachmentsRef.current = next;
    setAttachments(next);
  };

  const openAttachmentPicker = async () => {
    if (inputDisabled) return;
    const session = attachmentSessionRef.current;
    try {
      const selected = await open({ multiple: true, directory: false });
      const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      if (paths.length === 0) return;
      const inspected = await inspectChatAttachments(paths);
      if (session !== attachmentSessionRef.current) {
        discardPendingAttachments(inspected);
        return;
      }
      addAttachments(inspected);
    } catch (error) {
      if (session === attachmentSessionRef.current) {
        setAttachmentError(errorMessage(error, "添加附件失败。"));
      }
    }
  };

  const removeAttachment = (attachment: PendingChatAttachment) => {
    const next = attachmentsRef.current.filter((item) => item.id !== attachment.id);
    attachmentsRef.current = next;
    setAttachments(next);
    setAttachmentError("");
    void discardStagedChatAttachment(attachment.path);
  };

  const submitMessage = async () => {
    if (!canSend || !conversationId || preparingAttachmentsRef.current) return;
    const targetConversationId = conversationId;
    const session = attachmentSessionRef.current;
    const pending = attachmentsRef.current;
    const requestId = crypto.randomUUID();
    activeAttachmentTaskRef.current = requestId;
    preparingAttachmentsRef.current = true;
    setPreparingAttachments(true);
    setAttachmentError("");
    try {
      const storedAttachments = pending.length > 0
        ? await importChatAttachments(
            requestId,
            targetConversationId,
            pending.map((attachment) => attachment.path),
          )
        : [];
      if (
        session !== attachmentSessionRef.current
        || targetConversationId !== previousConversationIdRef.current
      ) {
        if (storedAttachments.length > 0) {
          await discardImportedChatAttachments(targetConversationId, storedAttachments);
        }
        return;
      }
      onSend(draft, storedAttachments);
      setDraft("");
      attachmentsRef.current = [];
      setAttachments([]);
    } catch (error) {
      if (session === attachmentSessionRef.current) {
        setAttachmentError(errorMessage(error, "保存附件失败。"));
      }
    } finally {
      if (activeAttachmentTaskRef.current === requestId) {
        activeAttachmentTaskRef.current = null;
      }
      preparingAttachmentsRef.current = false;
      setPreparingAttachments(false);
    }
  };

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void submitMessage();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) return;
    event.preventDefault();
    void submitMessage();
  };

  const handlePaste = async (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    if (inputDisabled) return;
    const files = Array.from(event.clipboardData.files).filter((file) => file.size > 0);
    if (files.length === 0) return;
    event.preventDefault();
    setAttachmentError("");
    const session = attachmentSessionRef.current;
    const pasted: PendingChatAttachment[] = [];
    try {
      for (const [index, file] of files.entries()) {
        const attachment = await savePastedChatAttachment(
          attachmentName(file, index),
          file.type || "application/octet-stream",
          await fileToBase64(file),
        );
        if (session !== attachmentSessionRef.current) {
          void discardStagedChatAttachment(attachment.path);
          discardPendingAttachments(pasted);
          return;
        }
        pasted.push(attachment);
      }
      addAttachments(pasted);
    } catch (error) {
      for (const attachment of pasted) {
        void discardStagedChatAttachment(attachment.path);
      }
      if (session === attachmentSessionRef.current) {
        setAttachmentError(errorMessage(error, "粘贴附件失败。"));
      }
    }
  };

  return (
    <footer className="composer-area">
      <div className="composer-inner">
        <form className="composer-box" onSubmit={handleSubmit}>
          <ChatAttachments
            attachments={attachments}
            variant="composer"
            onRemove={removeAttachment}
          />
          {attachmentError ? <p className="composer-attachment-error" role="alert">{attachmentError}</p> : null}
          <textarea
            className="composer-textarea"
            rows={2}
            placeholder={placeholder}
            aria-label="消息输入框"
            disabled={inputDisabled}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
          />

          <div className="composer-toolbar">
            <div className="composer-tools">
              <button
                className="icon-button"
                type="button"
                title="添加附件"
                aria-label="添加附件"
                disabled={inputDisabled}
                onClick={() => void openAttachmentPicker()}
              >
                <Paperclip size={18} />
              </button>
              <button className="icon-button" type="button" title="选择文献" disabled={inputDisabled}>
                <BookOpenText size={18} />
              </button>
              <button className="icon-button" type="button" title="对话选项" disabled={inputDisabled}>
                <SlidersHorizontal size={18} />
              </button>
            </div>

            <div className="composer-actions">
              <ContextUsageIndicator
                usage={contextUsage}
                contextWindowTokens={contextWindowTokens}
                messageCount={contextMessageCount}
                compressionCount={contextCompressionCount}
                disabled={contextDisabled}
                placement="up"
              />
              <button
                className={`send-button${busy && onStop ? " stop-button" : ""}`}
                type={busy && onStop ? "button" : "submit"}
                title={busy && onStop ? "停止生成" : "发送消息"}
                aria-label={busy && onStop ? "停止生成" : "发送消息"}
                disabled={busy && onStop ? stopDisabled : !canSend}
                onClick={busy && onStop ? onStop : undefined}
              >
                {busy && onStop
                  ? <Square size={15} fill="currentColor" />
                  : busy || preparingAttachments
                    ? <LoaderCircle className="composer-spin" size={18} />
                    : <ArrowUp size={19} />}
              </button>
            </div>
          </div>
        </form>

        <p className="composer-note">图片会发送给支持视觉的模型；其他文件目前只保存附件，不会解析正文。</p>
      </div>
    </footer>
  );
}
