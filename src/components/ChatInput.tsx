import { useState, type FormEvent, type KeyboardEvent } from "react";
import { ArrowUp, BookOpenText, Paperclip, SlidersHorizontal } from "lucide-react";
import "../styles/chat-input.css";

type ChatInputProps = {
  disabled?: boolean;
  onSend: (content: string) => void;
};

export function ChatInput({ disabled = false, onSend }: ChatInputProps) {
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
            placeholder={disabled ? "请先新建对话" : "向 Mnemora 提问..."}
            aria-label="消息输入框"
            disabled={disabled}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={handleKeyDown}
          />

          <div className="composer-toolbar">
            <div className="composer-tools">
              <button className="icon-button" type="button" title="添加附件">
                <Paperclip size={18} />
              </button>
              <button className="icon-button" type="button" title="选择文献">
                <BookOpenText size={18} />
              </button>
              <button className="icon-button" type="button" title="对话选项">
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
              <ArrowUp size={19} />
            </button>
          </div>
        </form>

        <p className="composer-note">Mnemora 可能会出错，请核对重要信息。</p>
      </div>
    </footer>
  );
}
