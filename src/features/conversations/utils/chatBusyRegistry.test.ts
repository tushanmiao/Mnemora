import { describe, expect, it } from "vitest";
import { createChatBusyRegistry } from "./chatBusyRegistry";

describe("createChatBusyRegistry", () => {
  it("keeps two conversations independent so a side window is never blocked by the main one", () => {
    // 这是副窗并发的核心前提：原来这是一个全局 boolean，主窗生成时副窗发送会被挡住。
    const registry = createChatBusyRegistry();
    registry.begin("main");

    expect(registry.isBusy("main")).toBe(true);
    expect(registry.isBusy("side")).toBe(false);
    expect(registry.isAnyBusy()).toBe(true);

    registry.begin("side");
    expect([...registry.busyConversationIds()].sort()).toEqual(["main", "side"]);

    registry.end("main");
    expect(registry.isBusy("main")).toBe(false);
    expect(registry.isBusy("side")).toBe(true);
    expect(registry.isAnyBusy()).toBe(true);

    registry.end("side");
    expect(registry.isAnyBusy()).toBe(false);
  });

  it("treats a missing conversation id as not busy", () => {
    const registry = createChatBusyRegistry();
    registry.begin("main");
    expect(registry.isBusy(null)).toBe(false);
    expect(registry.isBusy(undefined)).toBe(false);
    expect(registry.isBusy("")).toBe(false);
  });

  it("is idempotent so a retried run does not leak a busy slot", () => {
    const registry = createChatBusyRegistry();
    registry.begin("main");
    registry.begin("main");
    registry.end("main");
    expect(registry.isAnyBusy()).toBe(false);
  });
});
