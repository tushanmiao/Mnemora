import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  ArrowUp,
  BookOpenText,
  Command,
  FileText,
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
import type { ChatMessage, ChatQuote, LiteratureReference, NoteReference } from "../../../types/chat";
import type { LocalSlashCommand, SlashCommandExecutionResult } from "../commands/slashCommands";
import { buildSlashSuggestions, parseSlashInput } from "../commands/slashCommands";
import { resolveSkillActivation } from "../utils/skillActivation";
import { ChatAttachments } from "./ChatAttachments";
import { ContextUsageIndicator } from "./ContextUsageIndicator";
import { ActiveSkillTags, SkillPicker } from "./SkillPicker";
import { useI18n } from "../../../i18n/I18nProvider";
import { formatChatQuotes, MAX_CHAT_QUOTES } from "../utils/quotes";
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
  /** false 时保持普通 Chat，并禁止选择本轮 Skill。 */
  supportsTools?: boolean | null;
  /** Work 模式才显示文献入口；普通 Chat 不展示未实现的文献选择控件。 */
  showLiteraturePicker?: boolean;
  /** 从助手回答中选中的引用片段；发送时作为引用上下文并入消息。 */
  quotes?: ChatQuote[];
  /** 用户移除单条引用。 */
  onQuoteRemove?: (quoteId: string) => void;
  /** 用户清除全部引用或发送完成后调用。 */
  onQuotesClear?: () => void;
  /** Work 中待随本轮发送的 PDF 选区或单页引用。 */
  literatureReferences?: LiteratureReference[];
  onLiteratureReferenceRemove?: (referenceId: string) => void;
  onLiteratureReferencesClear?: () => void;
  /** Notes 中待随本轮发送的结构化 Markdown 选区引用。 */
  noteReferences?: NoteReference[];
  onNoteReferenceRemove?: (referenceId: string) => void;
  onNoteReferencesClear?: () => void;
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
    noteReferences?: NoteReference[],
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
  placeholder,
  focusRequest = 0,
  contextUsage,
  contextWindowTokens,
  supportsVision = null,
  supportsTools = null,
  showLiteraturePicker = false,
  quotes = [],
  onQuoteRemove,
  onQuotesClear,
  literatureReferences = [],
  onLiteratureReferenceRemove,
  onLiteratureReferencesClear,
  noteReferences = [],
  onNoteReferenceRemove,
  onNoteReferencesClear,
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
  const { t } = useI18n();
  const resolvedPlaceholder = placeholder ?? t("chat.placeholder");
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
    || noteReferences.length > 0
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
    const rejected = incoming.filter((attachment) => (
      (attachment.kind === "image" && supportsVision === false)
      || (attachment.kind !== "image" && supportsTools === false)
    ));
    const accepted = incoming.filter((attachment) => !rejected.includes(attachment));
    const capabilityErrors: string[] = [];
    if (rejected.some((attachment) => attachment.kind === "image")) {
      capabilityErrors.push(t("chat.visionUnsupportedDetail"));
    }
    if (rejected.some((attachment) => attachment.kind !== "image")) {
      capabilityErrors.push(t("chat.toolsUnsupportedDetail"));
    }
    discardPendingAttachments(rejected);
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
      setAttachmentError(t("chat.attachmentLimit", { count: MAX_ATTACHMENTS }));
      for (const attachment of unique.slice(available)) {
        void discardStagedChatAttachment(attachment.path);
      }
    } else {
      setAttachmentError(capabilityErrors.join(" "));
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
        setAttachmentError(errorMessage(error, t("chat.addAttachmentFailed")));
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
        setCommandFeedback(t("chat.unknownCommand", { command: parsedCommand.trigger }));
        return;
      }
    }
    if (parsedCommand?.kind === "conflict") {
      setCommandFeedback(t("chat.commandConflict", { command: parsedCommand.trigger }));
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
          setCommandFeedback(result.message ?? t("chat.commandNotExecuted"));
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
          onQuotesClear?.();
          onLiteratureReferencesClear?.();
          onNoteReferencesClear?.();
        }
      } catch (error) {
        setCommandFeedback(errorMessage(error, t("chat.commandFailed")));
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
      // 每条引用独立成块，避免多条选区在 Markdown 中互相粘连。
      const contentWithQuote = formatChatQuotes(quotes, draft);
      onSend(
        contentWithQuote,
        storedAttachments,
        resolveSkillActivation(draft, selectedSkillIds, skills),
        literatureReferences,
        noteReferences,
      );
      onQuotesClear?.();
      onLiteratureReferencesClear?.();
      onNoteReferencesClear?.();
      setDraft("");
      setCommandFeedback("");
      setUnknownSlashConfirmation(null);
      attachmentsRef.current = [];
      setAttachments([]);
    } catch (error) {
      if (session === attachmentSessionRef.current) {
        setAttachmentError(errorMessage(error, t("chat.saveAttachmentFailed")));
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
        setAttachmentError(errorMessage(error, t("chat.pasteAttachmentFailed")));
      }
    }
  };

  return (
    <footer className="composer-area">
      <div className="composer-inner">
        <form className="composer-box" onSubmit={handleSubmit}>
          {quotes.length > 0 ? (
            <div className="composer-quotes" aria-label={t("chat.quoteLabel")}>
              <div className="composer-quotes-header">
                <span className="composer-quotes-count">
                  <Quote size={14} />
                  {t("chat.quoteCount", { count: quotes.length, max: MAX_CHAT_QUOTES })}
                </span>
                <button
                  className="composer-quotes-clear"
                  type="button"
                  title={t("chat.clearQuotes")}
                  aria-label={t("chat.clearQuotes")}
                  onClick={() => onQuotesClear?.()}
                >
                  {t("chat.clearQuotes")}
                </button>
              </div>
              <div className="composer-quotes-list">
                {quotes.map((quote) => (
                  <div className="composer-quote-bar" key={quote.id}>
                    <span className="composer-quote-text">{quote.text}</span>
                    <button
                      className="composer-quote-clear"
                      type="button"
                      title={t("chat.removeQuote")}
                      aria-label={t("chat.removeQuote")}
                      onClick={() => onQuoteRemove?.(quote.id)}
                    >
                      <X size={14} />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
          {literatureReferences.length > 0 ? (
            <div className="composer-literature-references" aria-label={t("chat.literatureLabel")}>
              {literatureReferences.map((reference) => (
                <div className="composer-literature-reference" key={reference.id}>
                  <BookOpenText size={14} />
                  <span>
                    <strong title={reference.title}>{reference.title}</strong>
                    <small>
                      {t("chat.pageReference", {
                        page: reference.pageIndex + 1,
                        kind: t(reference.kind === "selection" ? "chat.selection" : "chat.currentPage"),
                      })}
                    </small>
                    <em title={reference.text}>{reference.text}</em>
                  </span>
                  <button
                    type="button"
                    title={t("chat.removeLiterature")}
                    aria-label={t("chat.removeLiteratureDetail", { title: reference.title, page: reference.pageIndex + 1 })}
                    onClick={() => onLiteratureReferenceRemove?.(reference.id)}
                  >
                    <X size={14} />
                  </button>
                </div>
              ))}
            </div>
          ) : null}
          {noteReferences.length > 0 ? (
            <div className="composer-note-references" aria-label="本轮笔记引用">
              {noteReferences.map((reference) => (
                <div className="composer-note-reference" key={reference.id}>
                  <FileText size={14} />
                  <span>
                    <strong title={reference.noteTitle}>{reference.noteTitle}</strong>
                    <small>{reference.startLine ? `第 ${reference.startLine}${reference.endLine && reference.endLine !== reference.startLine ? `-${reference.endLine}` : ""} 行` : "Markdown 选区"}</small>
                    <em title={reference.selectedText}>{reference.selectedText}</em>
                  </span>
                  <button
                    type="button"
                    title="移除笔记引用"
                    aria-label={`移除笔记引用 ${reference.noteTitle}`}
                    onClick={() => onNoteReferenceRemove?.(reference.id)}
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
            <div className="slash-command-menu" role="listbox" aria-label={t("chat.slashCommands")}>
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
            placeholder={resolvedPlaceholder}
            aria-label={t("chat.inputLabel")}
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
                title={supportsVision === false && supportsTools === false
                  ? t("chat.attachmentsUnsupported")
                  : supportsVision === false
                  ? t("chat.visionUnsupported")
                  : t("chat.addAttachment")}
                aria-label={supportsVision === false && supportsTools === false
                  ? t("chat.attachmentsUnsupported")
                  : supportsVision === false
                  ? `${t("chat.addAttachment")} (${t("chat.visionUnsupported")})`
                  : t("chat.addAttachment")}
                disabled={inputDisabled || (supportsVision === false && supportsTools === false)}
                onClick={() => void openAttachmentPicker()}
              >
                <Paperclip size={18} />
                {supportsVision === false ? (
                  <span className="document-only-badge" aria-hidden="true">{t("chat.documentOnly")}</span>
                ) : null}
              </button>
              {showLiteraturePicker ? (
                <button className="icon-button" type="button" title={t("chat.selectLiterature")} disabled={inputDisabled}>
                  <BookOpenText size={18} />
                </button>
              ) : null}
              <SkillPicker
                skills={skills}
                selectedSkillIds={selectedSkillIds}
                disabled={inputDisabled || supportsTools === false}
                disabledReason={supportsTools === false ? t("chat.toolsUnsupported") : undefined}
                onChange={onSelectedSkillsChange}
              />
              <button className="icon-button" type="button" title={t("chat.options")} disabled={inputDisabled}>
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
                title={busy && onStop ? t("chat.stop") : t("chat.send")}
                aria-label={busy && onStop ? t("chat.stop") : t("chat.send")}
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

        <p className="composer-note">{t("chat.attachmentsNote")}</p>
      </div>
    </footer>
  );
}
