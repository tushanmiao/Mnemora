import { chromium } from "@playwright/test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";

const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1366, height: 768 } });
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
try {
  await page.goto(`${process.env.NOTE_TEST_URL ?? "http://localhost:1422"}/scripts/notes/editor-harness.html`);
  await page.getByRole("textbox", { name: "Markdown 笔记正文", exact: true }).waitFor();
  const checkbox = page.getByRole("checkbox", { name: "切换任务状态" }).first();
  await checkbox.check();
  await page.waitForFunction(() => window.noteFixture.session.snapshot().content.includes("- [x] 核对来源"));
  await page.getByRole("button", { name: "阅读", exact: true }).click();
  const baseline = await page.evaluate(() => window.noteFixture.session.snapshot().content);
  await page.getByRole("button", { name: "查找替换", exact: true }).click();
  await page.locator(".cm-search").waitFor();
  const reader = page.getByRole("textbox", { name: "Markdown 笔记正文", exact: true });
  await reader.click(); await page.keyboard.press("Control+End"); await page.keyboard.type("Forbidden edit");
  assert.equal(await page.evaluate(() => window.noteFixture.session.snapshot().content), baseline);
  await page.getByRole("button", { name: "源码", exact: true }).click();
  await reader.click(); await page.keyboard.press("Control+End"); await page.keyboard.type("\nShared undo marker");
  await page.getByLabel("合成测试入口").selectOption("literature");
  await page.getByRole("textbox", { name: "Markdown 笔记正文", exact: true }).waitFor();
  await page.getByRole("button", { name: "撤销", exact: true }).click();
  await page.waitForFunction(() => !window.noteFixture.session.snapshot().content.includes("Shared undo marker"));
  const html = await page.evaluate(async () => {
    const { buildNoteHtml } = await import("/src/features/notes/editor/exportNote.ts");
    const content = window.noteFixture.session.snapshot().content + "\n\n" + "Long content\n\n".repeat(3000) + "\n\nFINAL_EXPORT_SENTINEL";
    return buildNoteHtml("fixture-note", "HTML export", content, document.body);
  });
  assert.ok(html.includes("FINAL_EXPORT_SENTINEL"));
  assert.ok(html.includes("katex"));
  assert.ok(html.includes("<svg"));
  assert.ok(html.includes("base64,"));
  assert.ok(!html.includes("<script"));
  const results = [];
  for (const size of [50 * 1024, 500 * 1024]) {
    await page.getByRole("button", { name: "源码", exact: true }).click();
    await page.evaluate((size) => {
      const heading = "# Benchmark\n\n", line = "一段合成研究文字和 regular text.\n\n";
      const encoder = new TextEncoder();
      const repeated = heading + line.repeat(Math.floor((size - encoder.encode(heading).length) / encoder.encode(line).length));
      const text = repeated + "x".repeat(size - encoder.encode(repeated).length);
      window.noteFixture.session.configure(false, 700);
      window.noteFixture.session.edit({ content: text });
    }, size);
    await page.waitForFunction(() => window.noteFixture.session.snapshot().content.startsWith("# Benchmark"));
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await reader.click(); await page.keyboard.press("Control+End");
    await page.evaluate(() => {
      window.keyPaintTimes = [];
      window.noteTiming = (event) => { if (event.key === "x") requestAnimationFrame(() => window.keyPaintTimes.push(performance.now() - event.timeStamp)); };
      document.addEventListener("keydown", window.noteTiming);
    });
    for (let index = 0; index < 20; index++) { await page.keyboard.press("x"); await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve))); }
    const result = await page.evaluate(() => {
      document.removeEventListener("keydown", window.noteTiming);
      const sorted = window.keyPaintTimes.sort((a, b) => a - b);
      return { bytes: new TextEncoder().encode(window.noteFixture.session.snapshot().content).length, p95InputFrameMs: sorted[Math.floor(sorted.length * 0.95)], count: sorted.length };
    });
    results.push(result);
  }
  assert.deepEqual(errors, []);
  await fs.mkdir(new URL("../../.artifacts/plan16/", import.meta.url), { recursive: true });
  await fs.writeFile(new URL("../../.artifacts/plan16/behavior-report.json", import.meta.url), JSON.stringify({ browser: browser.version(), result: "passed", samples: results, checks: ["task toggle", "read search remains immutable", "cross-host undo", "full HTML export", "inline fonts", "rendered Mermaid"] }, null, 2));
  console.log(JSON.stringify({ result: "passed", browser: browser.version(), samples: results }));
} finally { await browser.close(); }
