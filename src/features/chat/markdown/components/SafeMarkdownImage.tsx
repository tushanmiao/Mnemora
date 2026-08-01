import { useState } from "react";
import { ImageOff, X } from "lucide-react";
import { MARKDOWN_RENDER_LIMITS } from "../utils/renderLimits";
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
  const [expanded, setExpanded] = useState(false);
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
      onClick={() => setExpanded(true)}
    />
  );

  return (
    <>
      {image}
      {expanded ? (
        <div className="markdown-image-viewer" role="dialog" aria-label={alt} onClick={() => setExpanded(false)}>
          <button type="button" className="markdown-image-viewer-close" aria-label="关闭图片" onClick={() => setExpanded(false)}>
            <X size={18} />
          </button>
          <img
            src={src}
            alt={alt}
            className="markdown-image-viewer-image"
            loading="eager"
            decoding="async"
            style={{ maxWidth: "min(96vw, 1600px)", maxHeight: "90vh" }}
            onClick={(event) => event.stopPropagation()}
          />
        </div>
      ) : null}
    </>
  );
}
