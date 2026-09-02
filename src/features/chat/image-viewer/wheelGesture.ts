/**
 * 查看器的滚轮语义：裸滚轮平移，Ctrl+滚轮缩放。
 *
 * 这套分工来自图像/地图类工具的既有习惯（也是浏览器自身的 pinch-zoom 约定：
 * 触控板捏合会以 `ctrlKey=true` 的 wheel 事件到达）。所以「Ctrl 缩放」不只是
 * 我们的选择，它同时让触控板捏合自动可用。
 *
 * 抽成纯函数是为了能在没有 jsdom 的测试里直接验证判定与步长。
 */

/**
 * 手动缩放的下限。
 *
 * 「适应窗口」不受它约束：一张 1387px 宽的图放进 356px 的可视区需要 26%，
 * 硬卡在 50% 会让重置之后照样有滚动条。所以真正的下限是
 * `min(MIN_ZOOM, fitZoom)`，由调用方算出后传进 `clampZoom`。
 */
export const MIN_ZOOM = 0.5;
export const MAX_ZOOM = 3;

/**
 * 一格滚轮对应的缩放比例。
 *
 * 用乘法而不是加固定值：从 0.5 放到 1 和从 2 放到 2.5 在观感上不是同一件事，
 * 等比步进才让每一格「看起来一样多」。
 */
const ZOOM_STEP_RATIO = 1.12;

/** deltaMode 常量。事件对象来自宿主，不能假设 WheelEvent 全局存在。 */
const DELTA_MODE_LINE = 1;
const DELTA_MODE_PAGE = 2;

/** 行/页模式换算成像素的经验值，与主流浏览器的默认行高、视口步长一致。 */
const LINE_HEIGHT_PX = 16;
const PAGE_HEIGHT_PX = 400;

export type WheelInput = {
  deltaX: number;
  deltaY: number;
  deltaMode?: number;
  ctrlKey?: boolean;
  metaKey?: boolean;
};

export type WheelOutcome =
  | { kind: "zoom"; zoom: number }
  | { kind: "pan"; dx: number; dy: number }
  | { kind: "ignore" };

/** 把 deltaMode 归一到像素，否则 Firefox 的行模式滚一下只动 3 像素。 */
export function normalizeWheelDelta(value: number, deltaMode = 0) {
  if (!Number.isFinite(value)) return 0;
  if (deltaMode === DELTA_MODE_LINE) return value * LINE_HEIGHT_PX;
  if (deltaMode === DELTA_MODE_PAGE) return value * PAGE_HEIGHT_PX;
  return value;
}

/**
 * 把缩放夹进合法区间。
 *
 * `minZoom` 允许低于 `MIN_ZOOM`，用来容纳超宽图的「适应窗口」档位；调用方传入
 * 该图实际的 fitZoom，就不会出现「重置到适应窗口反而被 50% 卡住」。
 */
export function clampZoom(value: number, minZoom = MIN_ZOOM) {
  const lower = Math.min(minZoom, MIN_ZOOM);
  if (!Number.isFinite(value)) return lower;
  return Math.max(lower, Math.min(MAX_ZOOM, value));
}

/**
 * 判断一次滚轮该缩放还是平移。
 *
 * metaKey 一并算作缩放，让 macOS 上的 Cmd+滚轮也成立——虽然当前只发桌面 Windows，
 * 但这个分支不额外增加复杂度，而漏掉它会在 macOS 上表现为「缩放键没反应」。
 */
export function resolveWheelGesture(
  event: WheelInput,
  zoom: number,
  minZoom = MIN_ZOOM,
): WheelOutcome {
  const deltaY = normalizeWheelDelta(event.deltaY, event.deltaMode);
  const deltaX = normalizeWheelDelta(event.deltaX, event.deltaMode);

  if (event.ctrlKey || event.metaKey) {
    if (deltaY === 0) return { kind: "ignore" };
    // 向上滚（deltaY < 0）放大，与系统滚动方向一致。
    const steps = -deltaY / 100;
    const next = clampZoom(zoom * ZOOM_STEP_RATIO ** steps, minZoom);
    // 已经贴在边界上时不要返回 zoom，否则调用方会白白 preventDefault 掉一次滚动。
    return next === zoom ? { kind: "ignore" } : { kind: "zoom", zoom: next };
  }

  if (deltaX === 0 && deltaY === 0) return { kind: "ignore" };
  return { kind: "pan", dx: deltaX, dy: deltaY };
}
