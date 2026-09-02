/**
 * 图表导出：把查看器里的 SVG 落成用户能双击打开的文件。
 *
 * 为什么优先 PNG：SVG 依赖字体与 CSS 变量，发给别人常常变形；PNG 是所见即所得。
 * 但栅格化会失败——WebView2 里 `foreignObject` 常画不出来，画布也有面积上限——
 * 所以失败必须降级到 SVG 文本，而不是让用户点了下载什么都没发生。
 *
 * 这个模块只放不碰 DOM 的部分，DOM 那一层薄到不需要 jsdom 也能验证决策逻辑。
 */

/** 画布面积上限。WebView2 超过约 2^24 像素会静默返回空白位图，所以宁可缩小。 */
const MAX_RASTER_PIXELS = 16_777_216;

/** 单边上限。面积够但极端长条同样会失败（例如 60000×200 的时序图）。 */
const MAX_RASTER_EDGE = 8_192;

/** 目标放大倍数。2 倍在高分屏上够清晰，再高只是徒增体积。 */
const TARGET_RASTER_SCALE = 2;

export type DiagramExportFormat = "png" | "svg";

export type DiagramExportPayload = {
  format: DiagramExportFormat;
  /** 写盘用的 base64。PNG 是位图字节，SVG 是 UTF-8 文本的 base64。 */
  base64: string;
};

/**
 * 从 `createMermaidPreviewSource` 产出的 data: URL 还原 SVG 文本。
 *
 * 只认 `image/svg+xml`：查看器也用来看普通位图附件，那种情况没有可导出的矢量源，
 * 必须让调用方明确拿到 null 而不是一段乱码。
 */
export function decodeSvgDataUrl(source: string): string | null {
  const match = /^data:image\/svg\+xml([^,]*),(.*)$/is.exec(source.trim());
  if (!match) return null;
  const parameters = match[1]?.toLowerCase() ?? "";
  const body = match[2] ?? "";
  try {
    if (parameters.includes(";base64")) {
      return decodeBase64ToText(body);
    }
    return decodeURIComponent(body);
  } catch {
    return null;
  }
}

/**
 * 计算栅格化尺寸。
 *
 * 先按目标倍数放大，再被面积和单边两个上限压回来——两个都要，因为它们各自能被
 * 不同形状的图表单独突破。
 */
export function rasterTargetSize(width: number, height: number, scale = TARGET_RASTER_SCALE) {
  if (!(width > 0) || !(height > 0) || !Number.isFinite(width) || !Number.isFinite(height)) {
    return null;
  }
  let ratio = Math.max(1, scale);
  ratio = Math.min(ratio, MAX_RASTER_EDGE / width, MAX_RASTER_EDGE / height);
  ratio = Math.min(ratio, Math.sqrt(MAX_RASTER_PIXELS / (width * height)));
  // 上限比原始尺寸还小时不再强行放大，但也不缩到 0：至少留 1 像素。
  const targetWidth = Math.max(1, Math.floor(width * ratio));
  const targetHeight = Math.max(1, Math.floor(height * ratio));
  return { width: targetWidth, height: targetHeight, ratio };
}

/** 二进制转 base64。分块是必须的：一次性展开大数组会把调用栈打爆。 */
export function bytesToBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += CHUNK) {
    const chunk = bytes.subarray(offset, offset + CHUNK);
    binary += String.fromCharCode(...chunk);
  }
  return encodeBinaryToBase64(binary);
}

/** UTF-8 文本转 base64。先编码成字节再转，否则非 ASCII 会抛 InvalidCharacterError。 */
export function textToBase64(value: string): string {
  return bytesToBase64(new TextEncoder().encode(value));
}

/** base64 还原成字节。剪贴板要的是 Blob，绕不开这一步。 */
export function base64ToBytes(value: string): Uint8Array {
  const binary = decodeBase64ToBinary(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/**
 * 按导出格式给出文件名与保存对话框的过滤器。
 *
 * 名字里带格式后缀，用户连续导出两次不会互相覆盖。
 */
export function diagramSaveOptions(format: DiagramExportFormat, baseName: string) {
  const trimmed = baseName.trim().replace(/\.(png|svg)$/i, "") || "diagram";
  return {
    defaultPath: `${trimmed}.${format}`,
    filters: [{
      name: format === "png" ? "PNG 图片" : "SVG 矢量图",
      extensions: [format],
    }],
  };
}

function decodeBase64ToText(value: string): string {
  const binary = decodeBase64ToBinary(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return new TextDecoder().decode(bytes);
}

// atob/btoa 是 WebView 与 Node 18+ 共有的全局函数，所以两边都不需要 polyfill。
function encodeBinaryToBase64(binary: string): string {
  return btoa(binary);
}

function decodeBase64ToBinary(value: string): string {
  return atob(value);
}
