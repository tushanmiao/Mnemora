// 通过 CDP 打开探针页并取回控制台输出。
//
// 用 Edge（与 Tauri 的 WebView2 同源 Chromium）而不是 jsdom：canvas 与 SVG
// 栅格化在 jsdom 里根本不存在，只有真实引擎能回答「foreignObject 能不能画出来」。
const CDP_PORT = process.argv[2] ?? "9333";
const TARGET_URL = process.argv[3] ?? "http://localhost:1420/probe-rasterize.html";

const version = await (await fetch(`http://127.0.0.1:${CDP_PORT}/json/version`)).json();
const browserWs = version.webSocketDebuggerUrl;

const socket = new WebSocket(browserWs);
let nextId = 0;
const pending = new Map();
const sessions = new Map();

socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id !== undefined && pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error)));
    else resolve(message.result);
    return;
  }
  if (message.method === "Runtime.consoleAPICalled" || message.method === "Runtime.exceptionThrown") {
    const handler = sessions.get(message.sessionId);
    handler?.(message);
  }
});

await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});

function send(method, params = {}, sessionId) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
}

const { targetId } = await send("Target.createTarget", { url: "about:blank" });
const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });

const logs = [];
sessions.set(sessionId, (message) => {
  if (message.method === "Runtime.exceptionThrown") {
    const detail = message.params.exceptionDetails;
    logs.push(`[exception] ${detail.exception?.description ?? detail.text}`);
    return;
  }
  const text = message.params.args
    .map((arg) => arg.value ?? arg.description ?? JSON.stringify(arg.preview ?? {}))
    .join(" ");
  logs.push(`[${message.params.type}] ${text}`);
});

await send("Runtime.enable", {}, sessionId);
await send("Page.enable", {}, sessionId);
await send("Page.navigate", { url: TARGET_URL }, sessionId);

// 探针自己会把结果写进 <pre>，所以轮询它的文本直到出现结束标记。
const deadline = Date.now() + 60_000;
let output = "";
while (Date.now() < deadline) {
  await new Promise((resolve) => setTimeout(resolve, 1_000));
  const { result } = await send(
    "Runtime.evaluate",
    { expression: "document.getElementById('out')?.textContent ?? ''", returnByValue: true },
    sessionId,
  );
  output = result.value ?? "";
  if (output.includes("完成。")) break;
}

console.log("=========== 探针输出 ===========");
console.log(output || "(空)");
if (logs.length) {
  console.log("=========== 控制台 ===========");
  console.log(logs.join("\n"));
}

await send("Target.closeTarget", { targetId });
socket.close();
process.exit(output.includes("完成。") ? 0 : 1);
