import { describe, expect, it } from "vitest";
import { findWorkspaceView, SORTED_WORKSPACE_VIEWS, WORKSPACE_VIEWS } from "./viewRegistry";

describe("workspace view registry", () => {
  it("keeps stable unique entries for the current three views", () => {
    expect(WORKSPACE_VIEWS.map((view) => view.id)).toEqual(["chat", "notes", "work"]);
    expect(new Set(WORKSPACE_VIEWS.map((view) => view.id)).size).toBe(WORKSPACE_VIEWS.length);
    expect(WORKSPACE_VIEWS.every((view) => Boolean(view.labelKey))).toBe(true);
  });

  it("sorts activity entries by order and resolves definitions", () => {
    expect(SORTED_WORKSPACE_VIEWS.map((view) => view.id)).toEqual(["chat", "notes", "work"]);
    expect(findWorkspaceView("notes")?.contextSidebar).toBe(false);
    expect(findWorkspaceView("chat")?.aiPanel).toBe("primary");
    expect(findWorkspaceView("work")?.aiPanel).toBe("panel");
  });
});
