import type { ReactNode } from "react";
import { PanelRightClose } from "lucide-react";
import {
  PanelResizeHandle,
  type PanelResizeHandleProps,
} from "../../layout/components/PanelResizeHandle";
import "../styles/notes-workspace.css";

type NotesContextPanelProps = {
  chatPanel: ReactNode;
  onClose: () => void;
  resize: Omit<PanelResizeHandleProps, "edge" | "label">;
};

/** 只在用户主动打开 AI 时挂载共享 Chat，关闭后整个面板立即卸载。 */
export function NotesContextPanel({ chatPanel, onClose, resize }: NotesContextPanelProps) {
  return (
    <aside className="notes-context-panel" aria-label="笔记 AI 对话">
      <PanelResizeHandle {...resize} edge="left" label="调整笔记 AI 面板宽度" />
      <div className="notes-context-chat">{chatPanel}</div>
      <button className="notes-context-close" type="button" title="收起 AI" aria-label="收起 AI" onClick={onClose}>
        <PanelRightClose size={17} />
      </button>
    </aside>
  );
}
