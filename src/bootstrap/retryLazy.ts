import { lazy, type ComponentType, type LazyExoticComponent } from "react";

/**
 * Vite 开发热更新可能让浏览器保留一个已经失效的动态模块 URL。
 * 失败时只刷新一次模块地址，避免按需页面加载失败直接升级为全局白屏。
 */
export function retryLazy<P>(
  importer: () => Promise<{ default: ComponentType<P> }>,
): LazyExoticComponent<ComponentType<P>> {
  return lazy(async () => {
    try {
      return await importer();
    } catch (firstError) {
      if (typeof window === "undefined") throw firstError;
      await new Promise<void>((resolve) => window.setTimeout(resolve, 80));
      try {
        const url = new URL(window.location.href);
        url.searchParams.set("mnemora_module_retry", String(Date.now()));
        window.history.replaceState(window.history.state, "", url);
      } catch {
        // URL 状态更新失败时仍然继续第二次导入。
      }
      return importer();
    }
  });
}
