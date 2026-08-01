import { useState } from "react";
import { ChevronDown, ListTree } from "lucide-react";
import type { MarkdownOutlineItem } from "../utils/outline";
import "../styles/enhanced-markdown.css";

export function MessageOutline({ items }: { items: readonly MarkdownOutlineItem[] }) {
  const [open, setOpen] = useState(false);
  if (items.length < 3) return null;
  return (
    <nav className={`markdown-outline${open ? " markdown-outline-open" : ""}`} aria-label="回答目录">
      <button type="button" className="markdown-outline-toggle" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
        <ListTree size={15} />
        <span>本条回答目录</span>
        <ChevronDown size={14} />
      </button>
      {open ? (
        <ol>
          {items.map((item) => (
            <li key={item.id} style={{ paddingLeft: `${Math.max(0, item.level - 1) * 10}px` }}>
              <button type="button" onClick={() => document.getElementById(item.id)?.scrollIntoView({ block: "start", behavior: "smooth" })}>{item.title}</button>
            </li>
          ))}
        </ol>
      ) : null}
    </nav>
  );
}

