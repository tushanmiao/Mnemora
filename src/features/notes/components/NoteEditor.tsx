import {
  lazy,
  Suspense,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
} from "react";
import {
  ArrowLeft,
  Bot,
  Check,
  Copy,
  Eye,
  FileCode2,
  LoaderCircle,
  ListTree,
  PanelRightOpen,
  Quote,
  Trash2,
  WandSparkles,
} from "lucide-react";
import type { LibraryNote } from "../../library/types";
import type { MarkdownOutlineItem } from "../../chat/markdown/utils/outline";
import { PanelResizeHandle } from "../../layout/components/PanelResizeHandle";
import {
  OUTLINE_DEFAULT_WIDTH,
  OUTLINE_MAX_WIDTH,
  OUTLINE_MIN_WIDTH,
  type NotesLayout,
} from "../utils/notesWorkspace";
import { NoteSourcesBar } from "./NoteSourcesBar";
import type { MarkdownSourceEditorHandle } from "./MarkdownSourceEditor";

const MarkdownNotePreview = lazy(() => import("./MarkdownNotePreview"));
const MarkdownSourceEditor = lazy(() => import("./MarkdownSourceEditor").then((module) => ({
  default: module.MarkdownSourceEditor,
})));

export type NoteSelectionMenu = {
  left: number;
  top: number;
  text: string;
  startLine?: number;
  endLine?: number;
};

type NoteEditorProps = {
  activeNote: LibraryNote;
  title: string;
  content: string;
  mode: "source" | "preview";
  loading: boolean;
  saving: boolean;
  saved: boolean;
  error: string;
  chatOpen: boolean;
  chatBusy: boolean;
  notesLayout: NotesLayout;
  outline: MarkdownOutlineItem[];
  stats: { words: number; characters: number; readingMinutes: number };
  selectionMenu: NoteSelectionMenu | null;
  workspaceRef: RefObject<HTMLElement | null>;
  editorRef: RefObject<MarkdownSourceEditorHandle | null>;
  previewRef: RefObject<HTMLDivElement | null>;
  onTitleChange: (title: string) => void;
  onContentChange: (content: string) => void;
  onModeChange: (mode: "source" | "preview") => void;
  onClose: () => void;
  onDelete: () => void;
  onToggleChat: () => void;
  onToggleOutline: () => void;
  onOutlineJump: (item: MarkdownOutlineItem) => void;
  onOutlineWidthPreview: (width: number) => void;
  onOutlineWidthCommit: (width: number) => void;
  onSourceSelection: (event: ReactMouseEvent<HTMLElement>) => void;
  onPreviewSelection: (event: ReactMouseEvent<HTMLDivElement>) => void;
  onSelectionClear: () => void;
  onAskSelection: () => void;
  onEditSelection: () => void;
  onOpenSourceConversation?: (conversationId: string, messageId: string | null) => void;
};

/** 单篇笔记编辑器只负责展示和编辑交互，加载、保存与分组状态留在工作区容器。 */
export function NoteEditor({
  activeNote,
  title,
  content,
  mode,
  loading,
  saving,
  saved,
  error,
  chatOpen,
  chatBusy,
  notesLayout,
  outline,
  stats,
  selectionMenu,
  workspaceRef,
  editorRef,
  previewRef,
  onTitleChange,
  onContentChange,
  onModeChange,
  onClose,
  onDelete,
  onToggleChat,
  onToggleOutline,
  onOutlineJump,
  onOutlineWidthPreview,
  onOutlineWidthCommit,
  onSourceSelection,
  onPreviewSelection,
  onSelectionClear,
  onAskSelection,
  onEditSelection,
  onOpenSourceConversation,
}: NoteEditorProps) {
  return (
    <section
      className="notes-workspace"
      aria-label="笔记工作区"
      ref={workspaceRef}
      data-outline={notesLayout.outlineOpen ? "open" : "closed"}
      style={{ "--notes-outline-width": `${notesLayout.outlineWidth}px` } as CSSProperties}
    >
      {notesLayout.outlineOpen ? (
        <aside className="notes-outline-pane" aria-label="笔记大纲">
          <header><ListTree size={14} /><strong>大纲</strong><span>{outline.length}</span></header>
          <div>
            {outline.length === 0 ? (
              <p>没有检测到标题。使用 “#” 开头的标题行会出现在这里。</p>
            ) : outline.map((item) => (
              <button
                type="button"
                key={item.id}
                style={{ paddingLeft: `${10 + (item.level - 1) * 13}px` }}
                title={item.title}
                onClick={() => onOutlineJump(item)}
              >
                {item.title}
              </button>
            ))}
          </div>
          <PanelResizeHandle
            edge="right"
            value={notesLayout.outlineWidth}
            defaultValue={OUTLINE_DEFAULT_WIDTH}
            minValue={OUTLINE_MIN_WIDTH}
            maxValue={OUTLINE_MAX_WIDTH}
            label="调整大纲宽度"
            onPreview={onOutlineWidthPreview}
            onCommit={onOutlineWidthCommit}
          />
        </aside>
      ) : null}
      <main className="notes-editor-pane">
        <header className="notes-toolbar">
          <button type="button" className="notes-back-button" title="返回笔记列表" aria-label="返回笔记列表" onClick={onClose}>
            <ArrowLeft size={16} /><span>笔记</span>
          </button>
          <div className="notes-title-wrap">
            <FileCode2 size={16} />
            <input value={title} aria-label="笔记标题" onChange={(event) => onTitleChange(event.target.value)} />
          </div>
          <div className="notes-toolbar-actions">
            <button
              type="button"
              className={notesLayout.outlineOpen ? "is-active" : ""}
              title={notesLayout.outlineOpen ? "收起大纲" : "展开大纲"}
              aria-label={notesLayout.outlineOpen ? "收起大纲" : "展开大纲"}
              aria-pressed={notesLayout.outlineOpen}
              onClick={onToggleOutline}
            >
              <ListTree size={16} />
            </button>
            <div className="notes-mode-tabs" role="tablist" aria-label="编辑模式">
              <button type="button" role="tab" aria-selected={mode === "source"} className={mode === "source" ? "is-active" : ""} onClick={() => onModeChange("source")}>
                <FileCode2 size={14} /><span>Markdown</span>
              </button>
              <button type="button" role="tab" aria-selected={mode === "preview"} className={mode === "preview" ? "is-active" : ""} onClick={() => onModeChange("preview")}>
                <Eye size={14} /><span>预览</span>
              </button>
            </div>
            <button type="button" title="删除笔记" aria-label="删除笔记" onClick={onDelete}><Trash2 size={16} /></button>
            <button className={chatOpen ? "is-active" : ""} type="button" title={chatOpen ? "收起 AI" : "打开 AI"} aria-label={chatOpen ? "收起 AI" : "打开 AI"} onClick={onToggleChat}>
              {chatBusy ? <LoaderCircle className="is-spinning" size={16} /> : chatOpen ? <Bot size={16} /> : <PanelRightOpen size={16} />}
            </button>
          </div>
        </header>

        {error ? <div className="notes-error" role="alert">{error}</div> : null}
        <NoteSourcesBar noteId={activeNote.id} onOpenConversation={onOpenSourceConversation} />
        {loading ? (
          <div className="notes-empty" role="status"><LoaderCircle className="is-spinning" size={24} />正在加载笔记</div>
        ) : (
          <div className="notes-document-host">
            {mode === "source" ? (
              <Suspense fallback={<div className="notes-empty"><LoaderCircle className="is-spinning" size={20} />正在加载编辑器</div>}>
                <MarkdownSourceEditor
                  key={activeNote.id}
                  ref={editorRef}
                  value={content}
                  ariaLabel="Markdown 源码编辑器"
                  onChange={onContentChange}
                  onSelectionChange={onSelectionClear}
                  onMouseUp={onSourceSelection}
                />
              </Suspense>
            ) : (
              <div ref={previewRef} className="notes-preview-host" onMouseUp={onPreviewSelection}>
                <Suspense fallback={<div className="notes-empty"><LoaderCircle className="is-spinning" size={20} />正在加载预览</div>}>
                  <MarkdownNotePreview noteId={activeNote.id} content={content} />
                </Suspense>
              </div>
            )}
            {selectionMenu ? (
              <div className="notes-selection-menu" style={{ left: selectionMenu.left, top: selectionMenu.top }} onMouseDown={(event) => event.preventDefault()}>
                <button type="button" onClick={() => { void navigator.clipboard?.writeText(selectionMenu.text); onSelectionClear(); }}><Copy size={13} />复制</button>
                <button type="button" onClick={onAskSelection}><Quote size={13} />引用提问</button>
                <button type="button" onClick={onEditSelection}><WandSparkles size={13} />AI 修改</button>
              </div>
            ) : null}
          </div>
        )}
        <footer className="notes-statusbar">
          <span>{mode === "source" ? "Markdown 源码" : "渲染预览"}</span>
          <span>{saving ? "自动保存中" : saved ? <><Check size={12} />已保存</> : "自动保存"}</span>
          <span>{stats.words} 词</span><span>{stats.characters} 字符</span><span>约 {stats.readingMinutes} 分钟</span>
        </footer>
      </main>
    </section>
  );
}
