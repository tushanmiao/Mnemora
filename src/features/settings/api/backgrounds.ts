import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type BackgroundImage = {
  name: string;
  path: string;
  byteSize: number;
};

/** 带 asset URL 的背景图；`src` 可直接放进 CSS `url()`。 */
export type BackgroundImageAsset = BackgroundImage & { src: string };

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "avif", "gif"];

function withAssetSrc(image: BackgroundImage): BackgroundImageAsset {
  return { ...image, src: convertFileSrc(image.path) };
}

/**
 * 让用户选一张本地图片并复制进数据目录。
 *
 * 返回 null 表示用户取消了选择——这不是错误，调用方不该弹提示。
 */
export async function pickAndImportBackgroundImage(): Promise<BackgroundImageAsset | null> {
  if (!isTauri()) throw new Error("导入背景图需要在 Mnemora 桌面应用中操作。");
  const selected = await open({
    title: "选择背景图片",
    multiple: false,
    directory: false,
    filters: [{ name: "图片", extensions: IMAGE_EXTENSIONS }],
  });
  if (typeof selected !== "string") return null;
  const image = await invoke<BackgroundImage>("import_background_image", { sourcePath: selected });
  return withAssetSrc(image);
}

export async function listBackgroundImages(): Promise<BackgroundImageAsset[]> {
  if (!isTauri()) return [];
  const images = await invoke<BackgroundImage[]>("list_background_images");
  return images.map(withAssetSrc);
}

export async function removeBackgroundImage(name: string): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("remove_background_image", { name });
}

/**
 * 把一张背景图变成可用的 CSS background 值。
 *
 * `cover` + `center` + `no-repeat` 是壁纸的通用取值；末尾补一层 `--color-app`
 * 兜底，图片没加载出来时不会露出透明背景。
 */
export function backgroundCssForImage(src: string): string {
  return `url("${src}") center / cover no-repeat, var(--color-app)`;
}
