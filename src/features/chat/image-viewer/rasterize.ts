/**
 * 把 SVG data URL 栅格化成 PNG base64。
 *
 * 这一层碰 Image 与 canvas，所以逻辑刻意压到最薄：尺寸决策在 `diagramExport.ts`，
 * 这里只负责「解码 → 画 → 取字节」，并且**任何一步失败都返回 null**，让调用方
 * 降级到 SVG 而不是把异常抛给用户。
 */
import { bytesToBase64, rasterTargetSize } from "../markdown/utils/diagramExport";

/** 解码超时。卡住通常意味着 SVG 引用了取不到的外部资源，等下去不会好转。 */
const DECODE_TIMEOUT_MS = 8_000;

export type RasterizeResult = {
  base64: string;
  width: number;
  height: number;
};

export async function rasterizeSvgToPng(
  source: string,
  fallbackWidth: number,
  fallbackHeight: number,
): Promise<RasterizeResult | null> {
  if (typeof document === "undefined" || typeof Image === "undefined") return null;

  const image = await decodeImage(source);
  if (!image) return null;

  // naturalWidth 为 0 说明 SVG 没有内在尺寸，退回查看器测到的显示尺寸。
  const width = image.naturalWidth || fallbackWidth;
  const height = image.naturalHeight || fallbackHeight;
  const target = rasterTargetSize(width, height);
  if (!target) return null;

  const canvas = document.createElement("canvas");
  canvas.width = target.width;
  canvas.height = target.height;
  const context = canvas.getContext("2d");
  if (!context) return null;

  try {
    context.drawImage(image, 0, 0, target.width, target.height);
  } catch {
    return null;
  }

  const bytes = await canvasToPngBytes(canvas);
  if (!bytes) return null;
  return { base64: bytesToBase64(bytes), width: target.width, height: target.height };
}

function decodeImage(source: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const image = new Image();
    let settled = false;
    const finish = (value: HTMLImageElement | null) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      resolve(value);
    };
    const timer = window.setTimeout(() => finish(null), DECODE_TIMEOUT_MS);
    image.onload = () => finish(image);
    image.onerror = () => finish(null);
    image.src = source;
  });
}

/**
 * 取 PNG 字节。
 *
 * 走 `toBlob` 而不是 `toDataURL`：后者会把整张图先变成一段巨大的 base64 字符串，
 * 大图表容易直接顶到字符串上限。画布被污染时 `toBlob` 会抛，这里一并按失败处理。
 */
async function canvasToPngBytes(canvas: HTMLCanvasElement): Promise<Uint8Array | null> {
  const blob = await new Promise<Blob | null>((resolve) => {
    try {
      canvas.toBlob((value) => resolve(value), "image/png");
    } catch {
      resolve(null);
    }
  });
  if (!blob || blob.size === 0) return null;
  try {
    return new Uint8Array(await blob.arrayBuffer());
  } catch {
    return null;
  }
}
