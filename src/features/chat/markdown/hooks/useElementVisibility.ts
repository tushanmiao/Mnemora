import { useEffect, useRef, useState, type RefObject } from "react";

/**
 * 只有内容接近可视区域时才启动 Mermaid、代码高亮和图片大图等增强能力。
 *
 * Once an enhancement has been activated it remains active for the lifetime of
 * the mounted block. Virtualized message lists can move an item across the
 * observer boundary while correcting measured heights; turning the enhancement
 * off at that moment would replace a rendered diagram with source text and
 * create a visible layout feedback loop.
 */
export function useElementVisibility<T extends HTMLElement>(
  rootMargin = "240px",
): { ref: RefObject<T | null>; visible: boolean } {
  const ref = useRef<T | null>(null);
  const [visible, setVisible] = useState(() => typeof IntersectionObserver === "undefined");

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    if (typeof IntersectionObserver === "undefined") {
      setVisible(true);
      return;
    }

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) setVisible(true);
      },
      { rootMargin, threshold: 0.01 },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [rootMargin]);

  return { ref, visible };
}
