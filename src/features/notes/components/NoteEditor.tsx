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
  Copy,
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
import { NoteOutline } from "./NoteOutline";
import type { MarkdownSourceEditorHandle } from "./MarkdownSourceEditor";

import type { NoteEditorMode } from "../api/noteEditing";
const NoteMarkdownEditor = lazy(() => import("./NoteMarkdownEditor").then((module) => ({
  default: module.NoteMarkdownEditor,
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
  mode: NoteEditorMode;
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
  onModeChange: (mode: NoteEditorMode) => void;
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
            <NoteOutline items={outline} onJump={onOutlineJump} />
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
            <input value={title} aria-label="笔记标题" readOnly={mode === "read"} onChange={(event) => onTitleChange(event.target.value)} />
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
              <Suspense fallback={<div className="notes-empty"><LoaderCircle className="is-spinning" size={20} />正在加载编辑器</div>}>
                <NoteMarkdownEditor
                  key={activeNote.id}
                  ref={editorRef}
                  noteId={activeNote.id}
                  directoryPath={activeNote.directoryPath}
                  mode={mode}
                  onModeChange={onModeChange}
                  value={content}
                  onChange={onContentChange}
                  onSelectionChange={onSelectionClear}
                  onMouseUp={onSourceSelection}
                  previewRef={previewRef}
                  onPreviewMouseUp={onPreviewSelection}
                />
              </Suspense>
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
          <span>{mode === "source" ? "Markdown 源码" : mode === "live" ? "实时编辑" : "阅读"}</span>
          <span>{stats.words} 词</span><span>{stats.characters} 字符</span><span>约 {stats.readingMinutes} 分钟</span>
        </footer>
      </main>
    </section>
  );
}
