import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Cable, LoaderCircle, Pencil, Plus, RefreshCw, Save, Trash2, X } from "lucide-react";
import {
  listMcpServers,
  refreshMcpServer,
  removeMcpServer,
  setMcpServerEnabled,
  upsertMcpServer,
  type McpOverview,
  type McpServerConfig,
  type McpServerView,
} from "../api/mcp";
import "../styles/agent-capabilities-settings.css";

type TransportKind = "streamableHttp" | "stdio";

type EditorState = {
  originalId: string | null;
  id: string;
  name: string;
  enabled: boolean;
  transport: TransportKind;
  url: string;
  bearerToken: string;
  tokenTouched: boolean;
  hasBearerToken: boolean;
  command: string;
  args: string;
  cwd: string;
  env: string;
  allowedTools: string;
  autoApproveTools: string;
  startupTimeoutMs: number;
  callTimeoutMs: number;
  maxOutputChars: number;
  maxConcurrency: number;
};

const EMPTY_EDITOR: EditorState = {
  originalId: null,
  id: "",
  name: "",
  enabled: false,
  transport: "streamableHttp",
  url: "https://",
  bearerToken: "",
  tokenTouched: false,
  hasBearerToken: false,
  command: "",
  args: "",
  cwd: "",
  env: "",
  allowedTools: "",
  autoApproveTools: "",
  startupTimeoutMs: 15_000,
  callTimeoutMs: 90_000,
  maxOutputChars: 20_000,
  maxConcurrency: 1,
};

function parseList(value: string) {
  return [...new Set(value.split(/[\n,]/).map((item) => item.trim()).filter(Boolean))];
}

function parseEnv(value: string) {
  return Object.fromEntries(value.split("\n").map((line) => line.trim()).filter(Boolean).map((line) => {
    const separator = line.indexOf("=");
    if (separator < 1) throw new Error(`环境变量格式无效：${line}`);
    return [line.slice(0, separator).trim(), line.slice(separator + 1)];
  }));
}

function editServer(server: McpServerView): EditorState {
  return {
    originalId: server.id,
    id: server.id,
    name: server.name,
    enabled: server.enabled,
    transport: server.transport.type,
    url: server.transport.type === "streamableHttp" ? server.transport.url : "https://",
    bearerToken: "",
    tokenTouched: false,
    hasBearerToken: server.transport.type === "streamableHttp" && server.transport.hasBearerToken,
    command: server.transport.type === "stdio" ? server.transport.command : "",
    args: server.transport.type === "stdio" ? server.transport.args.join("\n") : "",
    cwd: server.transport.type === "stdio" ? server.transport.cwd ?? "" : "",
    env: server.transport.type === "stdio"
      ? Object.entries(server.transport.env).map(([key, value]) => `${key}=${value}`).join("\n")
      : "",
    allowedTools: server.allowedTools.join("\n"),
    autoApproveTools: server.autoApproveTools.join("\n"),
    startupTimeoutMs: server.startupTimeoutMs,
    callTimeoutMs: server.callTimeoutMs,
    maxOutputChars: server.maxOutputChars,
    maxConcurrency: server.maxConcurrency,
  };
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function McpSettingsPanel() {
  const [overview, setOverview] = useState<McpOverview>({ servers: [] });
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setError(null);
      setOverview(await listMcpServers());
    } catch (cause) {
      setError(errorText(cause));
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const editingPluginServer = useMemo(() => editor?.originalId
    ? overview.servers.find((server) => server.id === editor.originalId)?.pluginId ?? null
    : null, [editor, overview.servers]);

  const mutate = async (key: string, operation: () => Promise<unknown>, message?: string) => {
    setBusy(key);
    setError(null);
    setNotice(null);
    try {
      await operation();
      await load();
      if (message) setNotice(message);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(null);
    }
  };

  const save = async () => {
    if (!editor) return;
    let config: McpServerConfig;
    try {
      const allowedTools = parseList(editor.allowedTools);
      const autoApproveTools = parseList(editor.autoApproveTools);
      config = {
        id: editor.id.trim(),
        name: editor.name.trim(),
        enabled: editor.enabled,
        transport: editor.transport === "streamableHttp"
          ? { type: "streamableHttp", url: editor.url.trim(), hasBearerToken: editor.hasBearerToken }
          : {
              type: "stdio",
              command: editor.command.trim(),
              args: editor.args.split("\n").map((value) => value.trim()).filter(Boolean),
              cwd: editor.cwd.trim() || null,
              env: parseEnv(editor.env),
            },
        allowedTools,
        autoApproveTools,
        startupTimeoutMs: editor.startupTimeoutMs,
        callTimeoutMs: editor.callTimeoutMs,
        maxOutputChars: editor.maxOutputChars,
        maxConcurrency: editor.maxConcurrency,
        pluginId: editingPluginServer,
      };
    } catch (cause) {
      setError(errorText(cause));
      return;
    }
    await mutate("save", async () => {
      await upsertMcpServer(config, editor.tokenTouched ? editor.bearerToken : undefined);
      if (config.enabled) await refreshMcpServer(config.id);
      setEditor(null);
    }, "MCP 服务器配置已保存。");
  };

  const toggle = (server: McpServerView, enabled: boolean) => mutate(`toggle:${server.id}`, async () => {
    await setMcpServerEnabled(server.id, enabled);
    if (enabled) await refreshMcpServer(server.id);
  }, enabled ? "服务器已启用并刷新工具目录。" : "服务器已停用。");

  return (
    <div className="settings-content agent-capabilities-settings">
      <div className="settings-content-heading">
        <div><h2>MCP 服务器</h2><span>连接外部工具并动态加入 Agent 能力目录</span></div>
        <button className="settings-button settings-button-primary" type="button" onClick={() => setEditor({ ...EMPTY_EDITOR })}>
          <Plus size={15} />添加服务器
        </button>
      </div>

      <div className="agent-security-note">
        <AlertTriangle size={17} />
        <div><strong>外部工具默认不受信任</strong><span>服务器返回的只读、幂等和破坏性标注仅用于展示，不会自动降低审批等级。只有精确加入“自动批准”列表的工具才会跳过敏感操作确认。</span></div>
      </div>

      {error ? <div className="settings-feedback settings-feedback-error">{error}</div> : null}
      {notice ? <div className="settings-feedback settings-feedback-success">{notice}</div> : null}

      {editor ? (
        <section className="agent-editor-card">
          <header><div><strong>{editor.originalId ? "编辑服务器" : "添加 MCP 服务器"}</strong><span>Streamable HTTP 支持 HTTPS；本机环回地址可使用 HTTP。凭据只存入系统凭据管理器。</span></div><button className="agent-icon-button" type="button" aria-label="关闭编辑器" onClick={() => setEditor(null)}><X size={16} /></button></header>
          <div className="agent-form-grid">
            <label><span>服务器 ID</span><input className="settings-input" value={editor.id} disabled={Boolean(editor.originalId)} placeholder="my-mcp" onChange={(event) => setEditor({ ...editor, id: event.target.value })} /></label>
            <label><span>显示名称</span><input className="settings-input" value={editor.name} placeholder="My MCP" onChange={(event) => setEditor({ ...editor, name: event.target.value })} /></label>
            <label><span>传输方式</span><select className="settings-input settings-select" value={editor.transport} onChange={(event) => setEditor({ ...editor, transport: event.target.value as TransportKind })}><option value="streamableHttp">Streamable HTTP</option><option value="stdio">本地 stdio</option></select></label>
            <label className="agent-checkbox"><input type="checkbox" checked={editor.enabled} onChange={(event) => setEditor({ ...editor, enabled: event.target.checked })} /><span>保存后立即启用并发现工具</span></label>
            {editor.transport === "streamableHttp" ? (
              <>
                <label className="agent-form-wide"><span>Endpoint URL</span><input className="settings-input" value={editor.url} placeholder="https://example.com/mcp" onChange={(event) => setEditor({ ...editor, url: event.target.value })} /></label>
                <label className="agent-form-wide"><span>Bearer Token {editor.hasBearerToken && !editor.tokenTouched ? "（已安全保存；留空不变）" : "（可选）"}</span><div className="agent-inline-field"><input className="settings-input" type="password" autoComplete="new-password" value={editor.bearerToken} onChange={(event) => setEditor({ ...editor, bearerToken: event.target.value, tokenTouched: true })} />{editor.hasBearerToken ? <button className="settings-button settings-button-secondary" type="button" onClick={() => setEditor({ ...editor, bearerToken: "", tokenTouched: true, hasBearerToken: false })}>清除凭据</button> : null}</div></label>
              </>
            ) : (
              <>
                <label className="agent-form-wide"><span>可执行命令</span><input className="settings-input" value={editor.command} placeholder="npx" onChange={(event) => setEditor({ ...editor, command: event.target.value })} /></label>
                <label><span>参数（每行一个）</span><textarea className="agent-textarea" value={editor.args} placeholder="-y&#10;@example/mcp-server" onChange={(event) => setEditor({ ...editor, args: event.target.value })} /></label>
                <label><span>环境变量（KEY=value）</span><textarea className="agent-textarea" value={editor.env} onChange={(event) => setEditor({ ...editor, env: event.target.value })} /></label>
                <label className="agent-form-wide"><span>工作目录（可选，必须为绝对路径）</span><input className="settings-input" value={editor.cwd} onChange={(event) => setEditor({ ...editor, cwd: event.target.value })} /></label>
              </>
            )}
            <label><span>工具允许列表（空 = 全部）</span><textarea className="agent-textarea" value={editor.allowedTools} placeholder="每行一个远端工具名" onChange={(event) => setEditor({ ...editor, allowedTools: event.target.value })} /></label>
            <label><span>自动批准列表（精确匹配）</span><textarea className="agent-textarea" value={editor.autoApproveTools} placeholder="必须同时位于允许列表" onChange={(event) => setEditor({ ...editor, autoApproveTools: event.target.value })} /></label>
            <label><span>启动超时（ms）</span><input className="settings-input" type="number" min={1000} max={120000} value={editor.startupTimeoutMs} onChange={(event) => setEditor({ ...editor, startupTimeoutMs: Number(event.target.value) })} /></label>
            <label><span>调用超时（ms）</span><input className="settings-input" type="number" min={1000} max={600000} value={editor.callTimeoutMs} onChange={(event) => setEditor({ ...editor, callTimeoutMs: Number(event.target.value) })} /></label>
            <label><span>输出字符上限</span><input className="settings-input" type="number" min={1000} max={200000} value={editor.maxOutputChars} onChange={(event) => setEditor({ ...editor, maxOutputChars: Number(event.target.value) })} /></label>
            <label><span>最大并发</span><input className="settings-input" type="number" min={1} max={8} value={editor.maxConcurrency} onChange={(event) => setEditor({ ...editor, maxConcurrency: Number(event.target.value) })} /></label>
          </div>
          <footer><button className="settings-button settings-button-secondary" type="button" onClick={() => setEditor(null)}>取消</button><button className="settings-button settings-button-primary" type="button" disabled={busy === "save" || Boolean(editingPluginServer)} title={editingPluginServer ? "插件提供的服务器由插件清单管理" : undefined} onClick={() => void save()}>{busy === "save" ? <LoaderCircle className="settings-spin" size={15} /> : <Save size={15} />}保存</button></footer>
        </section>
      ) : null}

      <div className="agent-card-list">
        {overview.servers.length === 0 ? <div className="agent-empty-state"><Cable size={28} /><strong>尚未配置 MCP 服务器</strong><span>添加服务器后，启用并刷新工具目录，Agent 才能发现这些能力。</span></div> : overview.servers.map((server) => (
          <section className="agent-capability-card" key={server.id}>
            <header>
              <div className="agent-card-title"><span className={`agent-status-dot agent-status-${server.status.state}`} /><div><strong>{server.name}</strong><span>{server.id} · {server.transport.type === "streamableHttp" ? "HTTP" : "stdio"}{server.pluginId ? ` · 插件 ${server.pluginId}` : ""}</span></div></div>
              <label className="settings-switch-label"><input type="checkbox" checked={server.enabled} disabled={busy !== null} onChange={(event) => void toggle(server, event.target.checked)} />启用</label>
            </header>
            <div className="agent-card-meta"><span>状态：{server.status.state}</span><span>工具：{server.status.toolCount}</span>{server.status.lastSuccessAt ? <span>最近成功：{new Date(server.status.lastSuccessAt).toLocaleString()}</span> : null}</div>
            {server.status.lastError ? <div className="agent-card-error">{server.status.lastError}</div> : null}
            {server.tools.length > 0 ? <details className="agent-tool-list"><summary>查看 {server.tools.length} 个工具</summary>{server.tools.map((tool) => <div key={tool.wireName}><code>{tool.remoteName}</code><span>{tool.description}</span><small>{tool.readOnlyHint ? "声明只读" : "可能写入"} · {tool.autoApproved ? "已自动批准" : "调用前审批"}</small></div>)}</details> : null}
            <footer>
              <button className="settings-button settings-button-secondary" type="button" disabled={!server.enabled || busy !== null} onClick={() => void mutate(`refresh:${server.id}`, () => refreshMcpServer(server.id), "工具目录已刷新。")}><RefreshCw size={14} />刷新</button>
              <button className="settings-button settings-button-secondary" type="button" disabled={busy !== null || Boolean(server.pluginId)} onClick={() => setEditor(editServer(server))}><Pencil size={14} />编辑</button>
              <button className="settings-button settings-button-secondary agent-danger-button" type="button" disabled={busy !== null || Boolean(server.pluginId)} onClick={() => { if (window.confirm(`移除 MCP 服务器“${server.name}”？`)) void mutate(`remove:${server.id}`, () => removeMcpServer(server.id), "服务器已移除。"); }}><Trash2 size={14} />移除</button>
            </footer>
          </section>
        ))}
      </div>
    </div>
  );
}
