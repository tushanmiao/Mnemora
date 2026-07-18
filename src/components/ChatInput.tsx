import { useState, type FormEvent, type KeyboardEvent } from "react";
import {
  ArrowUp,
  BookOpenText,
  LoaderCircle,
  Paperclip,
  SlidersHorizontal,
} from "lucide-react";
import "../styles/chat-input.css";

type ChatInputProps = {
  disabled?: boolean;
  busy?: boolean;
  placeholder?: string;
  onSend: (content: string) => void;
};

export function ChatInput({
  disabled = false,
  busy = false,
  placeholder = "向 Mnemora 提问...",
  onSend,
}: ChatInputProps) {
  const [draft, setDraft] = useState("");
  const canSend = !disabled && draft.trim().length > 0;

  const submitMessage = () => {
    if (!canSend) return;

    onSend(draft);
    setDraft("");
  };

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    submitMessage();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) return;

    event.preventDefault();
    submitMessage();
  };

  return (
    <footer className="composer-area">
      <div className="composer-inner">
        <form className="composer-box" onSubmit={handleSubmit}>
          <textarea
            className="composer-textarea"
            rows={2}
            placeholder={placeholder}
            aria-label="消息输入框"
            disabled={disabled}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={handleKeyDown}
          />

          <div className="composer-toolbar">
            <div className="composer-tools">
              <button className="icon-button" type="button" title="添加附件" disabled={disabled}>
                <Paperclip size={18} />
              </button>
              <button className="icon-button" type="button" title="选择文献" disabled={disabled}>
                <BookOpenText size={18} />
              </button>
              <button className="icon-button" type="button" title="对话选项" disabled={disabled}>
                <SlidersHorizontal size={18} />
              </button>
            </div>

            <button
              className="send-button"
              type="submit"
              title="发送消息"
              aria-label="发送消息"
              disabled={!canSend}
            >
              {busy
                ? <LoaderCircle className="composer-spin" size={18} />
                : <ArrowUp size={19} />}
            </button>
          </div>
        </form>

        <p className="composer-note">Mnemora 可能会出错，请核对重要信息。</p>
      </div>
    </footer>
  );
}
