import { describe, expect, it } from "vitest";
import {
  heuristicSupportsVision,
  matchModelDefaults,
  resolveSupportsFunctionCalling,
  resolveSupportsReasoning,
  resolveSupportsVision,
} from "./modelMatching";

describe("matchModelDefaults", () => {
  it("精确匹配已收录模型", () => {
    const defaults = matchModelDefaults("gpt-5.5");
    expect(defaults).not.toBeNull();
    expect(defaults?.supportsVision).toBe(true);
    expect(defaults?.supportsFunctionCalling).toBe(true);
    expect(defaults?.supportsReasoning).toBe(true);
    expect(defaults?.contextWindowTokens).toBeGreaterThan(0);
    expect(defaults?.pricing?.currency).toBe("USD");
  });

  it("去掉 provider/ 前缀后匹配", () => {
    expect(matchModelDefaults("openai/gpt-5.5")?.supportsVision).toBe(true);
  });

  it("分隔符归一化：连字符版本号命中点号键", () => {
    expect(matchModelDefaults("gpt-5-5")?.supportsVision).toBe(
      matchModelDefaults("gpt-5.5")?.supportsVision,
    );
  });

  it("带日期后缀的模型名走前缀匹配", () => {
    expect(matchModelDefaults("gpt-5.5-2026-01-01")).not.toBeNull();
  });

  it("未收录模型返回 null", () => {
    expect(matchModelDefaults("totally-unknown-model-xyz")).toBeNull();
  });

  it("大小写不敏感", () => {
    expect(matchModelDefaults("GPT-5.5")?.supportsVision).toBe(true);
  });

  it("透出完整能力集合供徽章展示", () => {
    const capabilities = matchModelDefaults("gpt-5.5")?.capabilities;
    expect(capabilities).toBeDefined();
    expect(capabilities?.vision).toBe(true);
    expect(capabilities?.reasoning).toBe(true);
    expect(capabilities?.functionCalling).toBe(true);
  });

  it("识别 DeepSeek V4 和 Grok 4.6 的最新官方能力", () => {
    const flash = matchModelDefaults("deepseek-v4-flash");
    expect(flash?.contextWindowTokens).toBe(1_048_576);
    expect(flash?.supportsVision).toBe(false);
    expect(flash?.supportsFunctionCalling).toBe(true);
    expect(flash?.supportsReasoning).toBe(true);
    expect(flash?.pricing?.cacheReadPerMillion).toBe(0.0028);

    const pro = matchModelDefaults("deepseek-v4-pro");
    expect(pro?.supportsReasoning).toBe(true);
    expect(pro?.pricing?.cacheReadPerMillion).toBe(0.003625);

    const grok = matchModelDefaults("x-ai/grok-4.6-latest");
    expect(grok?.displayName).toBe("Grok 4.6");
    expect(grok?.contextWindowTokens).toBe(500_000);
    expect(grok?.supportsVision).toBe(true);
    expect(grok?.supportsFunctionCalling).toBe(true);
    expect(grok?.supportsReasoning).toBe(true);
    expect(grok?.pricing?.inputPerMillion).toBe(2);
    expect(grok?.pricing?.outputPerMillion).toBe(6);
  });
});

describe("heuristicSupportsVision", () => {
  it("DeepSeek 家族变体判为不支持（cherry-studio 行为）", () => {
    expect(heuristicSupportsVision("deepseek-v3.2-terminus")).toBe(false);
    expect(heuristicSupportsVision("DeepSeek-Coder-X")).toBe(false);
  });

  it("视觉标记优先于 DeepSeek 黑名单", () => {
    expect(heuristicSupportsVision("deepseek-vl2")).toBe(true);
  });

  it("视觉家族命中", () => {
    expect(heuristicSupportsVision("qwen9-vl-plus")).toBe(true);
    expect(heuristicSupportsVision("llava-next-13b")).toBe(true);
    expect(heuristicSupportsVision("some-vision-preview")).toBe(true);
  });

  it("非对话模型判为不支持", () => {
    expect(heuristicSupportsVision("text-embedding-9")).toBe(false);
    expect(heuristicSupportsVision("whisper-turbo")).toBe(false);
  });

  it("未知家族保持 undefined", () => {
    expect(heuristicSupportsVision("mystery-chat-model")).toBeUndefined();
  });
});

describe("resolveSupportsVision", () => {
  it("用户覆盖优先于数据库", () => {
    expect(resolveSupportsVision("gpt-5.5", false)).toBe(false);
    expect(resolveSupportsVision("totally-unknown-model-xyz", true)).toBe(true);
  });

  it("无覆盖时跟随数据库", () => {
    expect(resolveSupportsVision("gpt-5.5")).toBe(true);
  });

  it("数据库未命中时走家族启发式", () => {
    expect(resolveSupportsVision("deepseek-super-new-chat")).toBe(false);
  });

  it("两层都未知保持 undefined（放行）", () => {
    expect(resolveSupportsVision("mystery-chat-model")).toBeUndefined();
  });
});

describe("resolveSupportsFunctionCalling", () => {
  it("用户覆盖优先于数据库", () => {
    expect(resolveSupportsFunctionCalling("gpt-5.5", false)).toBe(false);
    expect(resolveSupportsFunctionCalling("unknown-relay-model", true)).toBe(true);
  });

  it("数据库命中时启用，未知模型保守关闭", () => {
    expect(resolveSupportsFunctionCalling("gpt-5.5")).toBe(true);
    expect(resolveSupportsFunctionCalling("unknown-relay-model")).toBe(false);
  });
});

describe("resolveSupportsReasoning", () => {
  it("解析数据库并允许用户覆盖", () => {
    expect(resolveSupportsReasoning("gpt-5.5")).toBe(true);
    expect(resolveSupportsReasoning("gpt-5.5", false)).toBe(false);
    expect(resolveSupportsReasoning("unknown-relay-model")).toBeUndefined();
  });
});
