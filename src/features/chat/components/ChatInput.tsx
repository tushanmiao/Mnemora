import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  ArrowUp,
  BookOpenText,
  Command,
  LoaderCircle,
  Paperclip,
  Quote,
  SlidersHorizontal,
  Square,
  X,
} from "lucide-react";
import type { ChatAttachment, PendingChatAttachment } from "../../../types/attachment";
import type { SkillActivationSelection, SkillSummary } from "../../../types/skill";
import {
  cancelChatAttachmentTask,
  discardImportedChatAttachments,
  discardStagedChatAttachment,
  importChatAttachments,
  inspectChatAttachments,
  savePastedChatAttachment,
} from "../api/attachments";
import type { ContextUsageEstimate } from "../utils/contextUsage";
import type { ChatMessage, LiteratureReference } from "../../../types/chat";
import type { LocalSlashCommand, SlashCommandExecutionResult } from "../commands/slashCommands";
import { buildSlashSuggestions, parseSlashInput } from "../commands/slashCommands";
import { resolveSkillActivation } from "../utils/skillActivation";
import { ChatAttachments } from "./ChatAttachments";
import { ContextUsageIndicator } from "./ContextUsageIndicator";
import { ActiveSkillTags, SkillPicker } from "./SkillPicker";
import "../styles/chat-input.css";

const MAX_ATTACHMENTS = 8;

type ChatInputProps = {
  conversationId: string | null;
  disabled?: boolean;
  busy?: boolean;
  stopDisabled?: boolean;
  placeholder?: string;
  /** 外部交互（例如 PDF 选区问 AI）请求聚焦输入框的递增序号。 */
  focusRequest?: number;
  contextUsage: ContextUsageEstimate;
  contextWindowTokens: number | null;
  /** 当前模型是否支持图片输入；false 时禁止添加图片附件，null 表示未知（放行）。 */
  supportsVision?: boolean | null;
  /** 从助手回答中选中的引用片段；发送时作为引用上下文并入消息。 */
  quote?: string | null;
  /** 用户移除引用条或发送完成后调用。 */
  onQuoteClear?: () => void;
  /** Work 中待随本轮发送的 PDF 选区或单页引用。 */
  literatureReferences?: LiteratureReference[];
  onLiteratureReferenceRemove?: (referenceId: string) => void;
  onLiteratureReferencesClear?: () => void;
  contextMessageCount: number;
  contextCompressionCount?: number;
  contextDisabled?: boolean;
  contextMessages?: ChatMessage[];
  contextSystemPrompt?: string;
  skills: SkillSummary[];
  selectedSkillIds: string[];
  onSelectedSkillsChange: (skillIds: string[]) => void;
  onSend: (
    content: string,
    attachments?: ChatAttachment[],
    skillActivation?: SkillActivationSelection,
    literatureReferences?: LiteratureReference[],
  ) => void;
  onStop?: () => void;
  onSlashCommand: (
    command: LocalSlashCommand,
    argumentsValue: string,
  ) => Promise<SlashCommandExecutionResult> | SlashCommandExecutionResult;
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
  focusRequest = 0,
  contextUsage,
  contextWindowTokens,
  supportsVision = null,
  quote = null,
  onQuoteClear,
  literatureReferences = [],
  onLiteratureReferenceRemove,
  onLiteratureReferencesClear,
  contextMessageCount,
  contextCompressionCount = 0,
  contextDisabled = false,
  contextMessages = [],
  contextSystemPrompt = "",
  skills,
  selectedSkillIds,
  onSelectedSkillsChange,
  onSend,
  onStop,
  onSlashCommand,
}: ChatInputProps) {
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<PendingChatAttachment[]>([]);
  const [attachmentError, setAttachmentError] = useState("");
  const [preparingAttachments, setPreparingAttachments] = useState(false);
  const [commandRunning, setCommandRunning] = useState(false);
  const [commandFeedback, setCommandFeedback] = useState("");
  const [unknownSlashConfirmation, setUnknownSlashConfirmation] = useState<string | null>(null);
  const [selectedCommandIndex, setSelectedCommandIndex] = useState(0);
  const preparingAttachmentsRef = useRef(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const attachmentsRef = useRef(attachments);
  const attachmentSessionRef = useRef(0);
  const activeAttachmentTaskRef = useRef<string | null>(null);
  const previousConversationIdRef = useRef(conversationId);
  attachmentsRef.current = attachments;
  const inputDisabled = disabled || busy || preparingAttachments || commandRunning;
  const canSend = !inputDisabled && (
    draft.trim().length > 0
    || attachments.length > 0
    || literatureReferences.length > 0
  );
  const slashSuggestions = useMemo(() => buildSlashSuggestions(draft, skills), [draft, skills]);
  const slashMenuOpen = !inputDisabled && draft.trimStart().startsWith("/") && !draft.includes("\n") && slashSuggestions.length > 0;

  useEffect(() => {
    if (focusRequest <= 0 || inputDisabled) return;
    const frame = requestAnimationFrame(() => textareaRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [focusRequest, inputDisabled]);

  useEffect(() => {
    setSelectedCommandIndex(0);
    setUnknownSlashConfirmation(null);
  }, [draft]);

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
    // 图片门禁：当前模型明确不支持视觉时，图片附件在入口处拒绝（与后端拦截一致）。
    let accepted = incoming;
    let visionError = "";
    if (supportsVision === false) {
      const rejectedImages = incoming.filter((attachment) => attachment.kind === "image");
      if (rejectedImages.length > 0) {
        accepted = incoming.filter((attachment) => attachment.kind !== "image");
        visionError = "当前模型不支持图片输入。请切换到支持视觉的模型，或在设置的模型能力中手动开启。";
        discardPendingAttachments(rejectedImages);
      }
    }
    const current = attachmentsRef.current;
    const existing = new Set(current.map((attachment) => attachment.path));
    const duplicates: PendingChatAttachment[] = [];
    const unique = accepted.filter((attachment) => {
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
      setAttachmentError(visionError);
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
    const parsedCommand = parseSlashInput(draft, skills);
    if (parsedCommand?.kind === "unknown") {
      if (unknownSlashConfirmation !== draft) {
        setUnknownSlashConfirmation(draft);
        setCommandFeedback(`未知命令：${parsedCommand.trigger}。再次按 Enter 可将它作为普通文本发送。`);
        return;
      }
    }
    if (parsedCommand?.kind === "conflict") {
      setCommandFeedback(`命令 ${parsedCommand.trigger} 被多个技能重复声明，请先在技能设置中解决冲突。`);
      return;
    }
    if (parsedCommand?.kind === "local") {
      if (parsedCommand.command === "attach") {
        setDraft("");
        setCommandFeedback("");
        setUnknownSlashConfirmation(null);
        await openAttachmentPicker();
        return;
      }
      setCommandRunning(true);
      setCommandFeedback("");
      try {
        const result = await onSlashCommand(parsedCommand.command, parsedCommand.arguments);
        if (!result.executed) {
          setCommandFeedback(result.message ?? "命令没有执行。");
          return;
        }
        setDraft(parsedCommand.command === "help" ? "/" : "");
        setCommandFeedback(parsedCommand.command === "help" ? "" : (result.message ?? ""));
        setUnknownSlashConfirmation(null);
        if (parsedCommand.command === "new" || parsedCommand.command === "clear") {
          const pending = attachmentsRef.current;
          attachmentsRef.current = [];
          setAttachments([]);
          discardPendingAttachments(pending);
        }
      } catch (error) {
        setCommandFeedback(errorMessage(error, "命令执行失败。"));
      } finally {
        setCommandRunning(false);
      }
      return;
    }
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
      // 引用片段以 Markdown 引用块并入消息开头：模型能天然理解 "> " 语义，
      // 会话历史里也保留了"针对哪段内容提问"的完整上下文。
      const quoted = quote?.trim();
      const contentWithQuote = quoted
        ? `${quoted.split(/\r?\n/).map((line) => `> ${line}`).join("\n")}\n\n${draft}`
        : draft;
      onSend(
        contentWithQuote,
        storedAttachments,
        resolveSkillActivation(draft, selectedSkillIds, skills),
        literatureReferences,
      );
      onQuoteClear?.();
      onLiteratureReferencesClear?.();
      setDraft("");
      setCommandFeedback("");
      setUnknownSlashConfirmation(null);
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
    if (slashMenuOpen && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      setSelectedCommandIndex((current) => (
        (current + direction + slashSuggestions.length) % slashSuggestions.length
      ));
      return;
    }
    if (slashMenuOpen && event.key === "Escape") {
      event.preventDefault();
      setDraft("");
      setCommandFeedback("");
      setUnknownSlashConfirmation(null);
      return;
    }
    if (slashMenuOpen && (event.key === "Tab" || event.key === "Enter") && !event.shiftKey) {
      const selected = slashSuggestions[selectedCommandIndex] ?? slashSuggestions[0];
      const currentToken = draft.trimStart().split(/\s+/, 1)[0]?.toLocaleLowerCase("en-US");
      if (selected && (event.key === "Tab" || currentToken !== selected.trigger)) {
        event.preventDefault();
        setDraft(`${selected.trigger} `);
        return;
      }
    }
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
          {quote ? (
            <div className="composer-quote-bar" aria-label="引用的回答片段">
              <Quote size={14} />
              <span className="composer-quote-text">{quote}</span>
              <button
                className="composer-quote-clear"
                type="button"
                title="移除引用"
                aria-label="移除引用"
                onClick={() => onQuoteClear?.()}
              >
                <X size={14} />
              </button>
            </div>
          ) : null}
          {literatureReferences.length > 0 ? (
            <div className="composer-literature-references" aria-label="本轮文献引用">
              {literatureReferences.map((reference) => (
                <div className="composer-literature-reference" key={reference.id}>
                  <BookOpenText size={14} />
                  <span>
                    <strong title={reference.title}>{reference.title}</strong>
                    <small>
                      第 {reference.pageIndex + 1} 页 · {reference.kind === "selection" ? "文字选区" : "当前页"}
                    </small>
                    <em title={reference.text}>{reference.text}</em>
                  </span>
                  <button
                    type="button"
                    title="移除文献引用"
                    aria-label={`移除 ${reference.title} 第 ${reference.pageIndex + 1} 页引用`}
                    onClick={() => onLiteratureReferenceRemove?.(reference.id)}
                  >
                    <X size={14} />
                  </button>
                </div>
              ))}
            </div>
          ) : null}
          <ChatAttachments
            attachments={attachments}
            variant="composer"
            onRemove={removeAttachment}
          />
          <ActiveSkillTags
            skills={skills}
            selectedSkillIds={selectedSkillIds}
            disabled={inputDisabled}
            onChange={onSelectedSkillsChange}
          />
          {attachmentError ? <p className="composer-attachment-error" role="alert">{attachmentError}</p> : null}
          {commandFeedback ? <p className="composer-command-feedback" role="status">{commandFeedback}</p> : null}
          {slashMenuOpen ? (
            <div className="slash-command-menu" role="listbox" aria-label="Slash 命令">
              {slashSuggestions.map((item, index) => (
                <button
                  type="button"
                  role="option"
                  aria-selected={index === selectedCommandIndex}
                  className={index === selectedCommandIndex ? "slash-command-active" : ""}
                  key={`${item.kind}:${item.skillId ?? item.trigger}:${item.trigger}`}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => {
                    setDraft(`${item.trigger} `);
                    setCommandFeedback("");
                    setUnknownSlashConfirmation(null);
                  }}
                >
                  <Command size={15} />
                  <span><strong>{item.trigger}</strong><small>{item.title} · {item.description}</small></span>
                  {item.argumentHint ? <em>{item.argumentHint}</em> : null}
                </button>
              ))}
            </div>
          ) : null}
          <textarea
            className="composer-textarea"
            ref={textareaRef}
            rows={2}
            placeholder={placeholder}
            aria-label="消息输入框"
            disabled={inputDisabled}
            value={draft}
            onChange={(event) => {
              setDraft(event.target.value);
              setCommandFeedback("");
              setUnknownSlashConfirmation(null);
            }}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
          />

          <div className="composer-toolbar">
            <div className="composer-tools">
              <button
                className={supportsVision === false ? "icon-button icon-button-document-only" : "icon-button"}
                type="button"
                title={supportsVision === false
                  ? "当前模型不支持图片，仅可添加文档附件"
                  : "添加附件"}
                aria-label={supportsVision === false
                  ? "添加附件（当前模型不支持图片，仅可添加文档）"
                  : "添加附件"}
                disabled={inputDisabled}
                onClick={() => void openAttachmentPicker()}
              >
                <Paperclip size={18} />
                {supportsVision === false ? (
                  <span className="document-only-badge" aria-hidden="true">仅文档</span>
                ) : null}
              </button>
              <button className="icon-button" type="button" title="选择文献" disabled={inputDisabled}>
                <BookOpenText size={18} />
              </button>
              <SkillPicker
                skills={skills}
                selectedSkillIds={selectedSkillIds}
                disabled={inputDisabled}
                onChange={onSelectedSkillsChange}
              />
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
                messages={contextMessages}
                systemPrompt={contextSystemPrompt}
                availableSkillCount={skills.filter((skill) => skill.enabled).length}
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
