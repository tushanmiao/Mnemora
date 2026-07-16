import { ArrowUp, BookOpenText, Paperclip, SlidersHorizontal } from "lucide-react";
import "../styles/chat-input.css";

export function ChatInput() {
  return (
    <footer className="composer-area">
      <div className="composer-inner">
        <div className="composer-box">
          <textarea
            className="composer-textarea"
            rows={2}
            placeholder="向 Mnemora 提问..."
            aria-label="消息输入框"
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

            <button className="send-button" type="button" title="发送消息" aria-label="发送消息">
              <ArrowUp size={19} />
            </button>
          </div>
        </div>

        <p className="composer-note">Mnemora 可能会出错，请核对重要信息。</p>
      </div>
    </footer>
  );
}
