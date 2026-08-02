import { useState } from "react";
import { ImageOff } from "lucide-react";
import { MARKDOWN_RENDER_LIMITS } from "../utils/renderLimits";
import { useImageViewer } from "../../image-viewer/ImageViewerContext";
import "../styles/enhanced-markdown.css";

type SafeMarkdownImageProps = {
  src?: string;
  alt?: string;
  title?: string;
  width?: string | number;
  height?: string | number;
};

function safeDimension(value: string | number | undefined) {
  if (value === undefined) return undefined;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 && parsed <= 4_096 ? parsed : undefined;
}

export function SafeMarkdownImage({ src, alt = "图片", title, width, height }: SafeMarkdownImageProps) {
  const [failed, setFailed] = useState(false);
  const { openImage } = useImageViewer();
  if (!src || failed) {
    return (
      <span className="markdown-image-fallback" role="img" aria-label={`${alt}加载失败`}>
        <ImageOff size={15} />
        <span>{alt}加载失败</span>
      </span>
    );
  }

  const image = (
    <img
      className="markdown-image"
      src={src}
      alt={alt}
      title={title}
      width={safeDimension(width)}
      height={safeDimension(height)}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onLoad={(event) => {
        const image = event.currentTarget;
        if (image.naturalWidth * image.naturalHeight > MARKDOWN_RENDER_LIMITS.maxDecodedImagePixels) {
          setFailed(true);
        }
      }}
      onError={() => setFailed(true)}
      onClick={() => openImage({ src, alt, title })}
    />
  );

  return (
    <>
      {image}
    </>
  );
}
