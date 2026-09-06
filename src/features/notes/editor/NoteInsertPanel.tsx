import { useState } from "react";
import { useNoteText } from "./noteText";
import type { FormatId } from "./commands";

export function NoteInsertPanel({ kind, onInsert, onClose }: { kind: "link" | "table" | "mermaid"; onInsert: (id: FormatId, argument: string) => void; onClose: () => void }) {
  const nt = useNoteText();
  const [url, setUrl] = useState("https://"), [rows, setRows] = useState(3), [columns, setColumns] = useState(3), [diagram, setDiagram] = useState("flowchart");
  const templates: Record<string, string> = {
    flowchart: "flowchart LR\n  A[Question] --> B[Evidence] --> C[Conclusion]",
    sequence: "sequenceDiagram\n  participant User\n  participant App\n  User->>App: Request\n  App-->>User: Response",
    state: "stateDiagram-v2\n  [*] --> Draft\n  Draft --> Saved\n  Saved --> [*]",
    mindmap: "mindmap\n  root((Topic))\n    Evidence\n    Questions\n    Conclusions",
  };
  const valid = kind !== "link" || /^(https?:\/\/[^\s<>]+|mailto:[^\s<>]+|#[^\s<>]+|attachments\/[^\r\n<>]+)$/.test(url);
  return <form className="note-insert-panel" onSubmit={(event) => {
    event.preventDefault(); if (!valid) return;
    onInsert(kind, kind === "link" ? url : kind === "table" ? `${rows}x${columns}` : templates[diagram]); onClose();
  }} onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); onClose(); } }}>
    {kind === "link" ? <label>{nt("链接地址")}<input autoFocus aria-label={nt("链接地址")} value={url} onChange={(event) => setUrl(event.target.value)} /></label> : kind === "table" ? <>
      <label>{nt("行数")}<input autoFocus type="number" aria-label={nt("行数")} min={1} max={200} value={rows} onChange={(event) => setRows(Number(event.target.value))} /></label>
      <label>{nt("列数")}<input type="number" aria-label={nt("列数")} min={1} max={30} value={columns} onChange={(event) => setColumns(Number(event.target.value))} /></label>
    </> : <label>{nt("图表类型")}<select autoFocus aria-label={nt("图表类型")} value={diagram} onChange={(event) => setDiagram(event.target.value)}>{Object.keys(templates).map((type) => <option key={type} value={type}>{type}</option>)}</select></label>}
    <button type="submit" disabled={!valid || kind === "table" && (rows * columns > 2000 || rows < 1 || columns < 1)}>{nt("插入")}</button>
    <button type="button" onClick={onClose}>{nt("取消")}</button>
  </form>;
}
