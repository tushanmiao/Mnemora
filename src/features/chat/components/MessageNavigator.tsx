import { useEffect, useRef, useState } from "react";
import type { MessageNavigatorNode } from "../utils/messageNavigator";
import "../styles/message-navigator.css";

type Props = {
  nodes: MessageNavigatorNode[];
  activeNodeId: string | null;
  onNavigate: (node: MessageNavigatorNode) => void;
  onNavigateStep: (direction: -1 | 1) => void;
};

type Preview = {
  node: MessageNavigatorNode;
  top: number;
  left: number;
};

export function MessageNavigator({ nodes, activeNodeId, onNavigate, onNavigateStep }: Props) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const lastWheelAtRef = useRef(0);

  useEffect(() => {
    if (!activeNodeId) return;
    const active = viewportRef.current?.querySelector<HTMLElement>(`[data-navigator-id="${CSS.escape(activeNodeId)}"]`);
    active?.scrollIntoView({ block: "nearest" });
  }, [activeNodeId]);

  const showPreview = (node: MessageNavigatorNode, button: HTMLButtonElement) => {
    const rect = button.getBoundingClientRect();
    const width = Math.min(300, window.innerWidth - 32);
    setPreview({
      node,
      top: Math.max(72, Math.min(window.innerHeight - 72, rect.top + rect.height / 2)),
      left: Math.max(16, rect.left - width - 10),
    });
  };

  return (
    <>
      <aside className="message-navigator" aria-label="对话轮次导航">
        <div
          className="message-navigator-viewport"
          ref={viewportRef}
          onMouseLeave={() => setPreview(null)}
          onWheel={(event) => {
            event.preventDefault();
            event.stopPropagation();
            if (Math.abs(event.deltaY) < 4) return;
            const now = performance.now();
            if (now - lastWheelAtRef.current < 140) return;
            lastWheelAtRef.current = now;
            onNavigateStep(event.deltaY > 0 ? 1 : -1);
          }}
        >
          <div className="message-navigator-track">
            {nodes.map((node, index) => {
              const active = node.id === activeNodeId;
              return (
                <button
                  className={`message-navigator-node${active ? " message-navigator-node-active" : ""}`}
                  type="button"
                  data-navigator-id={node.id}
                  aria-current={active ? "location" : undefined}
                  aria-label={`第 ${index + 1} 轮：${node.title}`}
                  key={node.id}
                  onClick={() => onNavigate(node)}
                  onMouseEnter={(event) => showPreview(node, event.currentTarget)}
                  onFocus={(event) => showPreview(node, event.currentTarget)}
                  onBlur={() => setPreview(null)}
                >
                  <span />
                </button>
              );
            })}
          </div>
        </div>
      </aside>
      {preview ? (
        <div className="message-navigator-preview" style={{ top: preview.top, left: preview.left }}>
          <strong>{preview.node.title}</strong>
          {preview.node.answerPreview ? <p>{preview.node.answerPreview}</p> : null}
          {preview.node.modelLabel ? <span>{preview.node.modelLabel}</span> : null}
        </div>
      ) : null}
    </>
  );
}
