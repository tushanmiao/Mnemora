import { describe, expect, it } from "vitest";
import { parseGitHubPackageSource } from "./remotePackages";

describe("parseGitHubPackageSource", () => {
  it("accepts owner/repo and repository URLs", () => {
    expect(parseGitHubPackageSource("openai/skills")).toEqual({ fullName: "openai/skills" });
    expect(parseGitHubPackageSource("https://github.com/openai/skills")).toEqual({
      fullName: "openai/skills",
    });
  });

  it("keeps the ref and package path from GitHub tree/blob URLs", () => {
    expect(parseGitHubPackageSource(
      "https://github.com/Nandansai08/skillz/tree/main/skills/research/research-question-framing",
    )).toEqual({
      fullName: "Nandansai08/skillz",
      gitRef: "main",
      packagePath: "skills/research/research-question-framing",
    });
    expect(parseGitHubPackageSource(
      "github.com/openai/skills/blob/main/skills/pdfs/SKILL.md",
    )).toEqual({
      fullName: "openai/skills",
      gitRef: "main",
      packagePath: "skills/pdfs/SKILL.md",
    });
  });

  it("rejects non-GitHub and ambiguous paths", () => {
    expect(parseGitHubPackageSource("https://example.com/openai/skills")).toBeNull();
    expect(parseGitHubPackageSource("openai/skills/extra")).toBeNull();
    expect(parseGitHubPackageSource("https://github.com/owner/repo/tree/main/%E0%A4%A")).toBeNull();
  });

  it("normalizes the optional git suffix", () => {
    expect(parseGitHubPackageSource("owner/repo.git")).toEqual({ fullName: "owner/repo" });
  });
});
