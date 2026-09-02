import { useCallback, useEffect, useState } from "react";
import { ImagePlus, LoaderCircle, Trash2 } from "lucide-react";
import {
  backgroundCssForImage,
  listBackgroundImages,
  pickAndImportBackgroundImage,
  removeBackgroundImage,
  type BackgroundImageAsset,
} from "../api/backgrounds";

type BackgroundImagePickerProps = {
  /** 当前的 background CSS，用来标出哪张图正在使用。 */
  currentCss: string;
  onSelect: (css: string) => void;
};

/**
 * 已导入背景图的缩略图网格 + 导入按钮。
 *
 * 图片复制进数据目录后只用 `asset://` URL 引用，所以设置导出里只有一个路径字符串，
 * 图片本身不会随配置外流——对肖像权敏感的素材这一点是必要的。
 */
export function BackgroundImagePicker({ currentCss, onSelect }: BackgroundImagePickerProps) {
  const [images, setImages] = useState<BackgroundImageAsset[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setImages(await listBackgroundImages());
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const importImage = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const imported = await pickAndImportBackgroundImage();
      // null 表示用户在对话框里取消了，不该当成错误。
      if (imported) {
        await reload();
        onSelect(backgroundCssForImage(imported.src));
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const removeImage = async (image: BackgroundImageAsset) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await removeBackgroundImage(image.name);
      await reload();
      // 删掉的正好是在用的那张：清空 CSS，否则界面会引用一个已不存在的文件。
      if (currentCss.includes(image.src)) onSelect("");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="background-picker">
      <div className="background-picker-actions">
        <button type="button" className="settings-button settings-button-secondary" disabled={busy} onClick={() => void importImage()}>
          {busy ? <LoaderCircle className="settings-spin" size={15} /> : <ImagePlus size={15} />}
          <span>选择图片</span>
        </button>
        <span className="background-picker-note">图片会复制到数据目录，不随设置导出</span>
      </div>
      {error ? <span className="theme-background-error">{error}</span> : null}
      {images.length > 0 ? (
        <ul className="background-picker-grid">
          {images.map((image) => {
            const active = currentCss.includes(image.src);
            return (
              <li key={image.name} data-active={active ? "true" : undefined}>
                <button
                  type="button"
                  className="background-picker-thumb"
                  style={{ backgroundImage: `url("${image.src}")` }}
                  aria-label={`使用背景图 ${image.name}`}
                  aria-pressed={active}
                  onClick={() => onSelect(backgroundCssForImage(image.src))}
                />
                <button
                  type="button"
                  className="background-picker-remove"
                  aria-label={`删除背景图 ${image.name}`}
                  disabled={busy}
                  onClick={() => void removeImage(image)}
                >
                  <Trash2 size={13} />
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}

export default BackgroundImagePicker;
