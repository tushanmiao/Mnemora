import { useNoteText } from "./noteText";
import { useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, AlignLeft, AlignCenter, AlignRight, Trash2, Code2, Copy } from "lucide-react";
import type { Table } from "mdast";
import { escapeTableCell, serializeTable, tableAt, tableCellRange, tableCells } from "./markdownRanges";

type CellPosition = [number, number];
type Props = {
  source: string; onChange: (next: string, structural?: boolean) => void; onSource: () => void;
  onUndo: () => void; onRedo: () => void; onSave: () => void; onComposition: (active: boolean) => void;
};
const withinBudget = (rows: number, columns: number) => rows <= 200 && columns <= 30 && rows * columns <= 2000;

export function NoteTableEditor({ source, onChange, onSource, onUndo, onRedo, onSave, onComposition }: Props) {
  const nt = useNoteText();
  const table = tableAt(source, 0);
  const host = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState<CellPosition>([0, 0]);
  const [extent, setExtent] = useState<CellPosition>([0, 0]);
  const [buffer, setBuffer] = useState<{ row: number; column: number; value: string } | null>(null);
  const [error, setError] = useState("");
  const composing = useRef(false), compositionCallback = useRef(onComposition);
  compositionCallback.current = onComposition;
  useEffect(() => () => { if (composing.current) compositionCallback.current(false); }, []);
  if (!table) return <pre>{source}</pre>;
  const rows = tableCells(source, table), width = rows[0].length;
  if (!withinBudget(rows.length, width)) return <button type="button" onClick={onSource}>{nt("表格超出可视编辑预算，打开源码")}</button>;
  for (const row of rows) while (row.length < width) row.push("");
  const rowIndex = Math.min(active[0], rows.length - 1), column = Math.min(active[1], width - 1);
  const bounds = { top: Math.min(rowIndex, extent[0]), bottom: Math.min(rows.length - 1, Math.max(rowIndex, extent[0])),
    left: Math.min(column, extent[1]), right: Math.min(width - 1, Math.max(column, extent[1])) };
  const rectangular = bounds.top !== bounds.bottom || bounds.left !== bounds.right;
  const selectedTsv = () => rows.slice(bounds.top, bounds.bottom + 1).map((row) => row.slice(bounds.left, bounds.right + 1).join("\t")).join("\n");
  const commit = (next: string[][], align = table.align) => {
    if (composing.current) return;
    if (!withinBudget(next.length, next[0].length)) { setError(nt("超出表格编辑预算；原文已保留，可在源码中粘贴。")); return; }
    setBuffer(null); setError(""); onChange(serializeTable(next, align), true);
  };
  const changeCell = (r: number, c: number, value: string) => {
    const cell = table.children[r]?.children[c];
    if (cell?.position) {
      const range = tableCellRange(source, cell);
      onChange(source.slice(0, range.from) + escapeTableCell(value) + source.slice(range.to));
    } else { rows[r][c] = value; commit(rows); }
  };
  const focusCell = (r: number, c: number) => requestAnimationFrame(() => {
    host.current?.querySelector<HTMLInputElement>(`input[data-row="${r}"][data-column="${c}"]`)?.focus();
  });
  const insertRow = (after: boolean) => { rows.splice(Math.min(rows.length, Math.max(1, rowIndex + Number(after))), 0, Array<string>(width).fill("")); commit(rows); };
  const insertColumn = (after: boolean) => {
    const index = column + Number(after), align = [...table.align ?? []];
    rows.forEach((row) => row.splice(index, 0, "")); align.splice(index, 0, null); commit(rows, align);
  };
  const alignment = (value: NonNullable<Table["align"]>[number]) => { const align = [...table.align ?? []]; align[column] = value; commit(rows, align); };
  return <div ref={host} className="note-table-editor" onKeyDown={(event) => {
    if (event.nativeEvent.isComposing || composing.current) return;
    if (event.ctrlKey || event.metaKey) {
      if (event.key.toLowerCase() === "z" || event.key.toLowerCase() === "y") {
        event.preventDefault(); event.stopPropagation();
        if (event.shiftKey || event.key.toLowerCase() === "y") onRedo(); else onUndo();
      }
      if (event.key.toLowerCase() === "s") { event.preventDefault(); event.stopPropagation(); onSave(); }
    }
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onSource(); }
  }} onCopy={(event) => {
    if (!rectangular) return;
    event.preventDefault(); event.stopPropagation(); event.clipboardData.setData("text/plain", selectedTsv());
  }}>
    <div className="note-block-tools" role="toolbar" aria-label={nt("表格操作")}>
      {([
        [nt("在上方插入行"), ArrowUp, () => insertRow(false), false], [nt("在下方插入行"), ArrowDown, () => insertRow(true), false],
        [nt("在左侧插入列"), ArrowLeft, () => insertColumn(false), false], [nt("在右侧插入列"), ArrowRight, () => insertColumn(true), false],
        [nt("左对齐"), AlignLeft, () => alignment("left"), false], [nt("居中"), AlignCenter, () => alignment("center"), false], [nt("右对齐"), AlignRight, () => alignment("right"), false],
        [nt("删除当前行"), Trash2, () => { rows.splice(rowIndex, 1); commit(rows); setActive([Math.max(0, rowIndex - 1), column]); setExtent([Math.max(0, rowIndex - 1), column]); }, rowIndex === 0],
        [nt("删除当前列"), Trash2, () => { rows.forEach((row) => row.splice(column, 1)); const align = [...table.align ?? []]; align.splice(column, 1); commit(rows, align); setActive([rowIndex, Math.max(0, column - 1)]); setExtent([rowIndex, Math.max(0, column - 1)]); }, width === 1],
        [nt("复制选中单元格"), Copy, () => { void navigator.clipboard.writeText(selectedTsv()).catch((error: unknown) => setError(String(error))); }, false],
        [nt("删除整表"), Trash2, () => onChange("", true), false],
        [nt("编辑表格源码"), Code2, onSource, false],
      ] as const).map(([label, Icon, action, disabled]) => <button type="button" key={label} aria-label={label} title={label} disabled={disabled || composing.current} onMouseDown={(event) => event.preventDefault()} onClick={action}><Icon size={14} /></button>)}
    </div>
    {error ? <p role="alert">{error}<button type="button" onClick={onSource}>{nt("打开源码")}</button></p> : null}
    <div className="markdown-table-scroll"><table><tbody>{rows.map((row, r) => <tr key={r}>{row.map((value, c) => {
      const Cell = r === 0 ? "th" : "td", cell = table.children[r]?.children[c];
      const range = cell?.position ? tableCellRange(source, cell) : null;
      return <Cell key={c} data-selected={rectangular && r >= bounds.top && r <= bounds.bottom && c >= bounds.left && c <= bounds.right ? "true" : undefined} style={{ textAlign: table.align?.[c] ?? undefined }}><input
        aria-label={nt(`${r === 0 ? "表头" : `第 ${r} 行`}，第 ${c + 1} 列`)}
        data-row={r} data-column={c} data-note-source-from={range?.from}
        value={buffer?.row === r && buffer.column === c ? buffer.value : value}
        onMouseDown={(event) => { if (event.shiftKey) { event.preventDefault(); setExtent([r, c]); } }}
        onFocus={() => { setActive([r, c]); setExtent([r, c]); }}
        onCompositionStart={() => { composing.current = true; onComposition(true); }}
        onCompositionEnd={(event) => {
          composing.current = false; changeCell(r, c, event.currentTarget.value);
          setBuffer(null); onComposition(false);
        }}
        onBlur={() => { if (!composing.current) setBuffer(null); }}
        onChange={(event) => {
          setBuffer({ row: r, column: c, value: event.target.value });
          if (!composing.current) changeCell(r, c, event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || composing.current) return;
          if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.startsWith("Arrow")) {
            event.preventDefault();
            const dr = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
            const dc = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
            setExtent(([y, x]) => [Math.max(0, Math.min(rows.length - 1, y + dr)), Math.max(0, Math.min(width - 1, x + dc))]);
            return;
          }
          if (event.key !== "Tab" && event.key !== "Enter") return;
          const next = r * width + c + (event.key === "Enter" ? width * (event.shiftKey ? -1 : 1) : event.shiftKey ? -1 : 1);
          if (next < 0) return;
          event.preventDefault(); event.stopPropagation();
          if (next >= rows.length * width) {
            if (event.key === "Enter" || !withinBudget(rows.length + 1, width)) { onSource(); return; }
            rows.push(Array<string>(width).fill("")); commit(rows);
          }
          focusCell(Math.floor(next / width), next % width);
        }}
        onPaste={(event) => {
          const text = event.clipboardData.getData("text/plain");
          if (!text.includes("\t") && !text.includes("\n")) return;
          event.preventDefault(); event.stopPropagation();
          if (text.length > 2 * 1024 * 1024) { setError(nt("粘贴内容超限，原文已保留。")); return; }
          const incoming = text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n").map((line) => line.split("\t"));
          const columns = Math.max(width, c + Math.max(...incoming.map((line) => line.length))), height = Math.max(rows.length, r + incoming.length);
          if (!withinBudget(height, columns)) { setError(nt("超出表格编辑预算；原文已保留，可在源码中粘贴。")); return; }
          while (rows.length < height) rows.push([]);
          rows.forEach((line) => { while (line.length < columns) line.push(""); });
          incoming.forEach((line, y) => line.forEach((cell, x) => { rows[r + y][c + x] = cell; })); commit(rows);
        }}
      /></Cell>;
    })}</tr>)}</tbody></table></div>
  </div>;
}
