import { useEffect, useRef, type KeyboardEvent, type PointerEvent } from "react";
import "../styles/panel-resize-handle.css";

export type PanelResizeEdge = "left" | "right";

export type PanelResizeHandleProps = {
  /** 面板哪一侧是拖动边界。left 表示向左拖动会增加面板宽度。 */
  edge: PanelResizeEdge;
  value: number;
  defaultValue: number;
  minValue: number;
  maxValue: number;
  label: string;
  getCurrentValue?: (handle: HTMLButtonElement) => number;
  getMaxValue?: (handle: HTMLButtonElement) => number;
  onPreview: (width: number) => void;
  onCommit: (width: number) => void;
};

type DragState = {
  pointerId: number;
  startX: number;
  startWidth: number;
  lastWidth: number;
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

function getElementWidth(handle: HTMLButtonElement): number {
  return handle.parentElement?.getBoundingClientRect().width ?? 0;
}

/**
 * 通用的左右面板拖动条。
 * 拖动期间只通过回调更新 CSS 变量，避免每个 pointermove 都触发 React 树重渲染。
 */
export function PanelResizeHandle({
  edge,
  value,
  defaultValue,
  minValue,
  maxValue,
  label,
  getCurrentValue = getElementWidth,
  getMaxValue,
  onPreview,
  onCommit,
}: PanelResizeHandleProps) {
  const handleRef = useRef<HTMLButtonElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const frameRef = useRef<number | null>(null);
  const pendingWidthRef = useRef<number | null>(null);

  const resolveMax = () => {
    const handle = handleRef.current;
    return getMaxValue && handle ? getMaxValue(handle) : maxValue;
  };

  const resolveCurrent = () => {
    const handle = handleRef.current;
    const current = handle ? getCurrentValue(handle) : value;
    return Number.isFinite(current) && current > 0 ? current : value;
  };

  const schedulePreview = (width: number) => {
    pendingWidthRef.current = width;
    if (frameRef.current !== null) return;
    frameRef.current = window.requestAnimationFrame(() => {
      frameRef.current = null;
      const nextWidth = pendingWidthRef.current;
      if (nextWidth === null) return;
      onPreview(nextWidth);
      handleRef.current?.setAttribute("aria-valuenow", String(Math.round(nextWidth)));
    });
  };

  const flushPreview = () => {
    if (frameRef.current !== null) {
      window.cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    }
    const nextWidth = pendingWidthRef.current;
    pendingWidthRef.current = null;
    if (nextWidth !== null) onPreview(nextWidth);
    return nextWidth;
  };

  const finishDrag = (commit: boolean) => {
    const drag = dragRef.current;
    if (!drag) return;
    const finalWidth = flushPreview() ?? drag.lastWidth;
    dragRef.current = null;
    document.body.classList.remove("panel-resizing");
    if (commit) onCommit(finalWidth);
  };

  const handlePointerDown = (event: PointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    const handle = event.currentTarget;
    const max = getMaxValue ? getMaxValue(handle) : maxValue;
    const startWidth = clamp(resolveCurrent(), minValue, max);
    event.preventDefault();
    handle.setPointerCapture(event.pointerId);
    handle.dataset.dragging = "true";
    document.body.classList.add("panel-resizing");
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth,
      lastWidth: startWidth,
    };
  };

  const handlePointerMove = (event: PointerEvent<HTMLButtonElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const direction = edge === "right" ? 1 : -1;
    const max = getMaxValue ? getMaxValue(event.currentTarget) : maxValue;
    const nextWidth = clamp(
      drag.startWidth + (event.clientX - drag.startX) * direction,
      minValue,
      max,
    );
    drag.lastWidth = nextWidth;
    schedulePreview(nextWidth);
  };

  const handlePointerUp = (event: PointerEvent<HTMLButtonElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    event.currentTarget.dataset.dragging = "false";
    finishDrag(true);
  };

  const handlePointerCancel = (event: PointerEvent<HTMLButtonElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    event.currentTarget.dataset.dragging = "false";
    finishDrag(true);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const max = resolveMax();
    const current = clamp(resolveCurrent(), minValue, max);
    let nextWidth: number | null = null;
    const step = event.shiftKey ? 32 : 12;
    if (event.key === "Home") nextWidth = minValue;
    if (event.key === "End") nextWidth = max;
    if (edge === "right" && event.key === "ArrowRight") nextWidth = current + step;
    if (edge === "right" && event.key === "ArrowLeft") nextWidth = current - step;
    if (edge === "left" && event.key === "ArrowLeft") nextWidth = current + step;
    if (edge === "left" && event.key === "ArrowRight") nextWidth = current - step;
    if (nextWidth === null) return;
    event.preventDefault();
    onPreview(clamp(nextWidth, minValue, max));
    onCommit(clamp(nextWidth, minValue, max));
  };

  const handleDoubleClick = () => {
    const nextWidth = clamp(defaultValue, minValue, resolveMax());
    onPreview(nextWidth);
    onCommit(nextWidth);
  };

  useEffect(() => () => {
    if (frameRef.current !== null) window.cancelAnimationFrame(frameRef.current);
    document.body.classList.remove("panel-resizing");
  }, []);

  return (
    <button
      ref={handleRef}
      className="panel-resize-handle"
      data-edge={edge}
      type="button"
      role="separator"
      aria-label={label}
      aria-orientation="vertical"
      aria-valuemin={minValue}
      aria-valuemax={maxValue}
      aria-valuenow={Math.round(value)}
      title="拖动调整宽度；双击恢复默认值"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerCancel}
      onKeyDown={handleKeyDown}
      onDoubleClick={handleDoubleClick}
    />
  );
}
