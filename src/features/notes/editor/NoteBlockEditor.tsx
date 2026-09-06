import { useNoteText } from "./noteText";
import { lazy, Suspense, useEffect, useId, useRef, useState } from "react";
import { Code2, Copy, Check, ImagePlus } from "lucide-react";
import { noteEditingApi } from "../api/noteEditing";
import { getNoteEditSession } from "../runtime/noteEditSession";
import { imageBase64 } from "./clipboard";

const Preview = lazy(() => import("../components/MarkdownNotePreview"));
type Props = {
  noteId: string; source: string; directoryPath?: string | null;
  onChange: (value: string, structural?: boolean) => void; onSource: () => void;
  onUndo: () => void; onRedo: () => void; onSave: () => void;
};

/** The local field is only an IME buffer; every completed edit is a CM transaction. */
export function NoteBlockEditor({ noteId, source, directoryPath, onChange, onSource, onUndo, onRedo, onSave }: Props) {
  const nt = useNoteText();
  const languageListId = `note-code-languages-${useId().replace(/:/g, "")}`;
  const [editing, setEditing] = useState(false), [buffer, setBuffer] = useState<string | null>(null);
  const [preview, setPreview] = useState(source), [error, setError] = useState("");
  const field = useRef<HTMLTextAreaElement>(null), composing = useRef(false), alive = useRef(true), latest = useRef(source);
  latest.current = source;
  const session = getNoteEditSession(noteId);
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; if (composing.current) session.composition(false); };
  }, [session]);
  useEffect(() => { const timer = setTimeout(() => setPreview(source), 400); return () => clearTimeout(timer); }, [source]);
  useEffect(() => { if (editing) field.current?.focus(); }, [editing]);
  const image = /^!\[([^\]]*)\]\(([^\n]+)\)$/.exec(source);
  const language = /^([`~]{3,})([^\n]*)\n/.exec(source);
  const replaceImage = async (file: File) => {
    const expected = source;
    session.pendingAssets++;
    try {
      const asset = await noteEditingApi.stageImage(noteId, session.sessionId, file.name, await imageBase64(file));
      if (!alive.current || latest.current !== expected) throw new Error(nt("图片已保留，但原引用已变化，请重新选择。"));
      onChange(`![${image?.[1] ?? ""}](${asset.relativePath})`, true);
      await session.checkpoint();
    } catch (error) { if (alive.current) setError(String(error)); }
    finally { session.pendingAssets--; }
  };
  return <div onDoubleClick={() => setEditing(true)}>
    <div className={editing ? "note-block-tools" : "note-block-source"}>
      <button type="button" onClick={() => { if (!composing.current) setEditing(!editing); }} aria-label={editing ? nt("完成块编辑") : nt("编辑块源码")} title={editing ? nt("完成块编辑") : nt("编辑块源码")}>{editing ? <Check size={14} /> : <Code2 size={14} />}</button>
      {editing ? <>
        <button type="button" aria-label={nt("复制块源码")} title={nt("复制块源码")} onClick={() => { void navigator.clipboard.writeText(source).catch((error: unknown) => setError(String(error))); }}><Copy size={14} /></button>
        <button type="button" onClick={onSource}>{nt("定位完整源码")}</button>
        {language ? <label>{nt("语言")}<input aria-label={nt("代码块语言")} value={language[2]} list={languageListId} onChange={(event) => {
          const next = event.target.value.replace(/[^\w+#.-]/g, "");
          onChange(source.slice(0, language[1].length) + next + source.slice(language[0].length - 1), true);
        }} /><datalist id={languageListId}>{["text", "python", "javascript", "typescript", "rust", "java", "sql", "json", "mermaid"].map((value) => <option value={value} key={value} />)}</datalist></label> : null}
        {image ? <>
          <label>Alt<input aria-label={nt("图片替代文字")} value={image[1]} onChange={(event) => onChange(`![${event.target.value.replace(/[\[\]\\\r\n]/g, "")}](${image[2]})`)} /></label>
          <label className="note-image-replace"><ImagePlus size={14} />{nt("替换图片")}<input aria-label={nt("替换图片")} type="file" accept="image/png,image/jpeg,image/webp,image/gif" onChange={(event) => {
            const file = event.target.files?.[0]; event.target.value = ""; if (file) void replaceImage(file);
          }} /></label>
        </> : null}
      </> : null}
    </div>
    {error ? <p role="alert">{error}</p> : null}
    {editing ? <textarea ref={field} className="note-technical-source" aria-label={nt("块 Markdown 源码")} data-note-source-from="0" spellCheck={false} value={buffer ?? source}
      onCompositionStart={() => { composing.current = true; session.composition(true); }}
      onCompositionEnd={(event) => { composing.current = false; onChange(event.currentTarget.value); setBuffer(null); session.composition(false); }}
      onChange={(event) => { if (composing.current) setBuffer(event.target.value); else onChange(event.target.value); }}
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing || composing.current) return;
        if ((event.ctrlKey || event.metaKey) && ["s", "z", "y"].includes(event.key.toLowerCase())) {
          event.preventDefault(); event.stopPropagation();
          if (event.key.toLowerCase() === "s") onSave();
          else if (event.shiftKey || event.key.toLowerCase() === "y") onRedo(); else onUndo();
        }
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setEditing(false); }
        if (event.key === "Tab" && !event.shiftKey) {
          event.preventDefault();
          const input = event.currentTarget, from = input.selectionStart, to = input.selectionEnd;
          onChange(source.slice(0, from) + "  " + source.slice(to));
          requestAnimationFrame(() => field.current?.setSelectionRange(from + 2, from + 2));
        }
      }} /> : null}
    <Suspense fallback={<pre>{source}</pre>}><Preview noteId={noteId} content={editing ? preview : source} directoryPath={directoryPath} fragment /></Suspense>
  </div>;
}
