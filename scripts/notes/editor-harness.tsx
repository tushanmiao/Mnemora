import React, { useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { NoteEditor } from "../../src/features/notes/components/NoteEditor";
import { NoteWorkspace } from "../../src/features/notes/components/NoteWorkspace";
import { noteEditingApi } from "../../src/features/notes/api/noteEditing";
import { useNoteEditSession, getNoteEditSession } from "../../src/features/notes/runtime/noteEditSession";
import { noteOutline } from "../../src/features/notes/editor/markdownRanges";
import { revisionHash, noteStats } from "../../src/features/notes/utils/notesWorkspace";
import { ImageViewerProvider } from "../../src/features/chat/image-viewer/ImageViewerContext";
import { I18nProvider } from "../../src/i18n/I18nProvider";
import "../../src/styles/tokens.css";
import "../../src/styles/themes.css";
import "../../src/features/notes/styles/notes-workspace.css";
import "./editor-harness.css";

const fixture = "# 研究笔记：Markdown 编辑验收\n\n这是一份合成测试资料。包含 **重点**、*术语*、[文献链接](https://example.org) 与 `变量`。\n\n## 证据比较\n\n| 方法 | 数据 | 结论 |\n| :--- | ---: | :---: |\n| 方法 A | 42 | 支持 |\n| 方法 B | 18 | 待验证 |\n\n## 数学表达\n\n$$\nE = mc^2\n$$\n\n## 实验流程\n\n```mermaid\nflowchart LR\n  A[问题] --> B[证据] --> C[结论]\n```\n\n## 代码\n\n```python\ndef evaluate(data):\n    return len(data)\n```\n\n## 任务\n\n- [ ] 核对来源\n- [x] 整理笔记\n\n结束。\n";
const id = "fixture-note";
const note = { id, itemId: null, itemTitle: null, title: "研究笔记 · 合成验收样本", content: fixture, createdAt: 1, updatedAt: 1, groupName: null, directoryPath: null, attachments: [] };
let base = { note, noteVersion: "1", contentHash: revisionHash(note), diskHash: revisionHash(note), externalContent: null, sourceMissing: false, drafts: [] };
const receipts = new Map();
const history = [];
let saves = 0;
noteEditingApi.load = async () => structuredClone(base);
noteEditingApi.checkpoint = async () => {};
noteEditingApi.discard = async () => {};
noteEditingApi.versions = async () => [...history];
noteEditingApi.save = async (request) => {
  if (receipts.has(request.operationId)) return receipts.get(request.operationId);
  if (request.expectedNoteVersion !== base.noteVersion) throw "NOTE_VERSION_CONFLICT";
  history.unshift({ id: crypto.randomUUID(), title: base.note.title, content: base.note.content, contentHash: base.contentHash, createdAt: Date.now(), reason: request.reason, pinned: false });
  saves++;
  base = { ...base, note: { ...base.note, title: request.title, content: request.markdown, updatedAt: Date.now() }, noteVersion: String(saves + 1), contentHash: revisionHash({ ...note, content: request.markdown }), diskHash: revisionHash({ ...note, content: request.markdown }) };
  const receipt = { operationId: request.operationId, draftGeneration: request.draftGeneration, noteId: id, noteVersion: base.noteVersion, contentHash: base.contentHash, title: request.title, committedMarkdown: request.markdown, updatedAt: base.note.updatedAt };
  receipts.set(request.operationId, receipt); return receipt;
};
window.noteFixture = { session: getNoteEditSession(id), saved: () => structuredClone(base), saves: () => saves };
const noop = () => {};
function Harness() {
  const [language, setLanguage] = useState("zh");
  const [surface, setSurface] = useState("ordinary"), [mode, setMode] = useState("live");
  const [layout, setLayout] = useState({ outlineOpen: true, outlineWidth: 200 });
  const editing = useNoteEditSession(id), editor = useRef(null), preview = useRef(null), workspace = useRef(null);
  return <I18nProvider language={language}><div className="app-shell harness" data-theme="light" data-theme-preset="graphite">
    <div className="fixture-control"><label>合成测试入口 <select value={surface} onChange={(event) => setSurface(event.target.value)}><option value="ordinary">普通笔记</option><option value="literature">文献笔记</option></select></label><label>Language <select aria-label="Language" value={language} onChange={(event) => setLanguage(event.target.value)}><option value="zh">中文</option><option value="en">English</option></select></label><span>独立内存资料，不连接真实资料库</span></div>
    <div className="fixture-workspace">{surface === "ordinary" ? editing.base ? <NoteEditor
      activeNote={editing.base.note} title={editing.title} content={editing.content} mode={mode} loading={false} saving={editing.phase === "saving"} saved={!editing.session?.dirty} error=""
      chatOpen={false} chatBusy={false} notesLayout={layout} outline={noteOutline(editing.content, `note-${id}`)} stats={noteStats(editing.content)} selectionMenu={null}
      workspaceRef={workspace} editorRef={editor} previewRef={preview} onTitleChange={(title) => editing.session?.edit({ title })} onContentChange={(content) => editing.session?.edit({ content })}
      onModeChange={setMode} onClose={noop} onDelete={noop} onToggleChat={noop} onToggleOutline={() => setLayout({ ...layout, outlineOpen: !layout.outlineOpen })}
      onOutlineJump={(item) => { editor.current?.setSelection(item.offset); editor.current?.scrollToLine(editing.content.slice(0, item.offset).split("\n").length); }}
      onOutlineWidthPreview={noop} onOutlineWidthCommit={noop} onSourceSelection={noop} onPreviewSelection={noop} onSelectionClear={noop} onAskSelection={noop} onEditSelection={noop}
    /> : <p>Loading</p> : <NoteWorkspace noteId={id} source={null} chatOpen={false} chatBusy={false} refreshVersion={0} onUpdated={noop} onDeleted={noop} onToggleChat={noop} onAskSelection={noop} onEditSelection={noop} onContextChange={noop} />}</div>
  </div></I18nProvider>;
}
createRoot(document.getElementById("root")!).render(<ImageViewerProvider><Harness /></ImageViewerProvider>);
