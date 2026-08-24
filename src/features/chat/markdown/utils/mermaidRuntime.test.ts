import { afterEach, describe, expect, it, vi } from "vitest";
import { waitForMermaidFonts } from "./mermaidRuntime";

describe("mermaidRuntime", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("loads the configured diagram font before Mermaid measures labels", async () => {
    const load = vi.fn(() => Promise.resolve([]));
    vi.stubGlobal("document", {
      fonts: {
        ready: Promise.resolve(),
        load,
      },
    });
    vi.stubGlobal("window", {
      setTimeout,
      clearTimeout,
    });

    await waitForMermaidFonts({
      themeVariables: {
        fontFamily: '"Microsoft YaHei UI", sans-serif',
        fontSize: "13px",
      },
    });

    expect(load).toHaveBeenCalledWith(
      '13px "Microsoft YaHei UI", sans-serif',
      "数据库 Database 表 Table",
    );
  });
});
