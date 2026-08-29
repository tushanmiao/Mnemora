import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ArchiveRestore, FolderOpen, LoaderCircle, Plug, RotateCcw, ShieldAlert, Trash2 } from "lucide-react";
import {
  installPlugin,
  listPlugins,
  rollbackPlugin,
  setPluginEnabled,
  uninstallPlugin,
  type PluginImportKind,
  type PluginOverview,
  type PluginSummary,
} from "../api/plugins";
import "../styles/agent-capabilities-settings.css";

type Props = { onSkillsChanged: () => Promise<unknown> | unknown };

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function PluginSettingsPanel({ onSkillsChanged }: Props) {
  const [overview, setOverview] = useState<PluginOverview>({ plugins: [], warnings: [] });
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setError(null);
      setOverview(await listPlugins());
    } catch (cause) {
      setError(errorText(cause));
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const mutate = async (key: string, operation: () => Promise<unknown>, message: string, refreshSkills = false) => {
    setBusy(key);
    setError(null);
    setNotice(null);
    try {
      await operation();
      await load();
      if (refreshSkills) await onSkillsChanged();
      setNotice(message);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(null);
    }
  };

  const chooseAndInstall = async (kind: PluginImportKind) => {
    const path = await open(kind === "directory"
      ? { title: "选择 Mnemora 插件目录", multiple: false, directory: true }
      : { title: "选择 Mnemora 插件 ZIP", multiple: false, directory: false, filters: [{ name: "ZIP", extensions: ["zip"] }] });
    if (typeof path !== "string") return;
    if (!window.confirm("插件签名验证尚未接入可信发布者目录。继续安装会把此包视为未验证代码与配置。请只安装你信任来源的插件。")) return;
    setBusy("install");
    setError(null);
    setNotice(null);
    try {
      try {
        await installPlugin(path, kind, false, true);
      } catch (cause) {
        const message = errorText(cause);
        if (!message.includes("already installed") || !window.confirm("该插件已安装。是否保存当前版本作为回滚副本并更新？")) throw cause;
        await installPlugin(path, kind, true, true);
      }
      await load();
      setNotice("插件已安装但尚未启用。请核对权限后再启用。");
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(null);
    }
  };

  const toggle = (plugin: PluginSummary, enabled: boolean) => mutate(
    `toggle:${plugin.id}`,
    () => setPluginEnabled(plugin.id, enabled),
    enabled ? "插件已启用。它提供的 MCP 服务器仍需在 MCP 设置中单独授权。" : "插件已停用，其技能与 MCP 能力已卸载。",
    true,
  );

  return (
    <div className="settings-content agent-capabilities-settings">
      <div className="settings-content-heading">
        <div><h2>插件</h2><span>安装声明式能力包：Skill 与远程 MCP 配置</span></div>
        <div className="settings-heading-actions">
          <button className="settings-button settings-button-secondary" type="button" disabled={busy !== null} onClick={() => void chooseAndInstall("directory")}><FolderOpen size={15} />安装目录</button>
          <button className="settings-button settings-button-primary" type="button" disabled={busy !== null} onClick={() => void chooseAndInstall("zip")}>{busy === "install" ? <LoaderCircle className="settings-spin" size={15} /> : <ArchiveRestore size={15} />}安装 ZIP</button>
        </div>
      </div>

      <div className="agent-security-note">
        <ShieldAlert size={17} />
        <div><strong>安装与启用分离</strong><span>插件安装后保持停用。启用时才物化 Skill 和 MCP 配置；插件声明的远程 MCP 服务器仍默认关闭，需要在 MCP 页面再次授权。当前仅支持声明式插件，禁止插件直接贡献本地可执行 stdio MCP。</span></div>
      </div>

      {error ? <div className="settings-feedback settings-feedback-error">{error}</div> : null}
      {notice ? <div className="settings-feedback settings-feedback-success">{notice}</div> : null}
      {overview.warnings.map((warning) => <div className="settings-feedback settings-feedback-error" key={warning}>{warning}</div>)}

      <div className="agent-card-list">
        {overview.plugins.length === 0 ? <div className="agent-empty-state"><Plug size={28} /><strong>尚未安装插件</strong><span>插件包必须包含严格的 plugin.json v1 清单。目录和 ZIP 都会经过路径、大小、数量与哈希校验。</span></div> : overview.plugins.map((plugin) => (
          <section className="agent-capability-card" key={plugin.id}>
            <header>
              <div className="agent-card-title"><span className={`agent-status-dot ${plugin.enabled ? "agent-status-ready" : "agent-status-disabled"}`} /><div><strong>{plugin.name} <small>v{plugin.version}</small></strong><span>{plugin.id} · {plugin.publisher}</span></div></div>
              <label className="settings-switch-label"><input type="checkbox" checked={plugin.enabled} disabled={busy !== null} onChange={(event) => void toggle(plugin, event.target.checked)} />启用</label>
            </header>
            {plugin.description ? <p className="agent-card-description">{plugin.description}</p> : null}
            <div className="agent-card-meta"><span>签名：{plugin.signatureStatus === "unsigned" ? "未签名" : "未验证"}</span><span>Skills：{plugin.skillIds.length}</span><span>MCP：{plugin.mcpServerIds.length}</span></div>
            <details className="agent-tool-list"><summary>查看能力与权限</summary><div><code>Skills</code><span>{plugin.skillIds.join(", ") || "无"}</span></div><div><code>MCP servers</code><span>{plugin.mcpServerIds.join(", ") || "无"}</span></div><div><code>网络域名</code><span>{plugin.permissions.networkDomains.join(", ") || "无"}</span></div><div><code>秘密权限</code><span>{plugin.permissions.secrets.join(", ") || "无"}</span></div></details>
            <footer>
              <button className="settings-button settings-button-secondary" type="button" disabled={plugin.enabled || !plugin.rollbackVersion || busy !== null} title={plugin.enabled ? "先停用插件" : plugin.rollbackVersion ? undefined : "没有回滚版本"} onClick={() => { if (window.confirm(`回滚“${plugin.name}”到 v${plugin.rollbackVersion}？`)) void mutate(`rollback:${plugin.id}`, () => rollbackPlugin(plugin.id), "插件已回滚，保持停用。", true); }}><RotateCcw size={14} />回滚{plugin.rollbackVersion ? `到 v${plugin.rollbackVersion}` : ""}</button>
              <button className="settings-button settings-button-secondary agent-danger-button" type="button" disabled={plugin.enabled || busy !== null} title={plugin.enabled ? "先停用插件" : undefined} onClick={() => { if (window.confirm(`卸载插件“${plugin.name}”？已保存的回滚副本也会被删除。`)) void mutate(`uninstall:${plugin.id}`, () => uninstallPlugin(plugin.id), "插件已卸载。", true); }}><Trash2 size={14} />卸载</button>
            </footer>
          </section>
        ))}
      </div>
    </div>
  );
}
