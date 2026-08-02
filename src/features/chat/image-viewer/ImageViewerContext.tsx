import { createContext, lazy, Suspense, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import type { ImageViewerItem } from "./types";

const ImageViewer = lazy(() => import("./ImageViewer").then(
  (module) => ({ default: module.ImageViewer }),
));

type ImageViewerContextValue = {
  openImage: (item: ImageViewerItem) => void;
  closeImage: () => void;
};

const ImageViewerContext = createContext<ImageViewerContextValue>({
  openImage: () => undefined,
  closeImage: () => undefined,
});

export function ImageViewerProvider({ children }: { children: ReactNode }) {
  const [item, setItem] = useState<ImageViewerItem | null>(null);
  const openImage = useCallback((nextItem: ImageViewerItem) => setItem(nextItem), []);
  const closeImage = useCallback(() => setItem(null), []);
  const value = useMemo(() => ({ openImage, closeImage }), [closeImage, openImage]);

  return (
    <ImageViewerContext.Provider value={value}>
      {children}
      {item ? (
        <Suspense fallback={null}>
          <ImageViewer item={item} onClose={closeImage} />
        </Suspense>
      ) : null}
    </ImageViewerContext.Provider>
  );
}

export function useImageViewer() {
  return useContext(ImageViewerContext);
}
