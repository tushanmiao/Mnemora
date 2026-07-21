import { describe, expect, it } from "vitest";
import { buildSandboxDocument } from "./sandboxDocument";

describe("HTML preview sandbox document", () => {
  it("removes active content and injects a restrictive CSP", () => {
    const output = buildSandboxDocument(`<!doctype html>
      <html><head><meta http-equiv="refresh" content="0;url=https://example.com"><style>.card{color:red}</style></head>
      <body><div class="card" onclick="bad()">safe</div><script>bad()</script><iframe src="https://example.com"></iframe><form action="https://example.com"><input></form></body></html>`);

    expect(output).toContain("Content-Security-Policy");
    expect(output).toContain("script-src 'none'");
    expect(output).toContain('<div class="card">safe</div>');
    expect(output).toContain(".card{color:red}");
    expect(output).not.toContain("http-equiv=\"refresh\"");
    expect(output).not.toContain("onclick");
    expect(output).not.toContain("<script");
    expect(output).not.toContain("bad()");
    expect(output).not.toContain("<iframe");
    expect(output).not.toContain("<form");
    expect(output).not.toContain("<input");
  });
});
