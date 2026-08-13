import { describe, expect, it } from "vitest";
import {
  allowedAttachmentExtensions,
  attachmentCapabilityError,
  classifyAttachment,
} from "./attachmentCapabilities";

const image = {
  kind: "image" as const,
  name: "figure.png",
  mimeType: "image/png",
};

const document = {
  kind: "file" as const,
  name: "paper.pdf",
  mimeType: "application/pdf",
};

describe("attachment capability gate", () => {
  it("allows unknown vision but rejects images explicitly marked unsupported", () => {
    expect(attachmentCapabilityError(image, null, false)).toBeNull();
    expect(attachmentCapabilityError(image, false, true)).toBe("vision");
  });

  it("requires explicit tool support for document attachments", () => {
    expect(attachmentCapabilityError(document, true, true)).toBeNull();
    expect(attachmentCapabilityError(document, true, false)).toBe("tools");
    expect(attachmentCapabilityError(document, true, null)).toBe("tools");
  });

  it("rejects formats without a registered safe reader", () => {
    expect(attachmentCapabilityError({
      kind: "file",
      name: "legacy.doc",
      mimeType: "application/msword",
    }, true, true)).toBe("format");
    expect(classifyAttachment("source.ts", "application/octet-stream")).toBe("document");
  });

  it("builds the file picker filter from the active capability snapshot", () => {
    expect(allowedAttachmentExtensions(false, false)).toEqual([]);
    expect(allowedAttachmentExtensions(true, false)).toEqual([
      "png", "jpg", "jpeg", "webp", "gif",
    ]);
    const documentOnly = allowedAttachmentExtensions(false, true);
    expect(documentOnly).toContain("pdf");
    expect(documentOnly).toContain("docx");
    expect(documentOnly).toContain("xlsx");
    expect(documentOnly).not.toContain("png");
  });
});
