import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  ArchiveRestore,
  ExternalLink,
  LoaderCircle,
  Search,
  ShieldAlert,
  Star,
  X,
} from "lucide-react";
import {
  fetchRemotePackage,
  formatBytes,
  formatRelativeDate,
  installRemotePackage,
  searchRemotePackages,
  type RemoteCandidate,
  type RemotePackageKind,
  type RemotePackagePreview,
  type RemoteSearchResult,
} from "../api/remotePackages";
// 这个对话框由 App 直接渲染，不在 SettingsPage 内，因此必须自己带上依赖的
// 样式：用户可能从未打开过设置页就在 Chat 里敲 /install-plugin github …，
// 那时 settings-kit / settings-page 都还没被加载，控件会退化成无样式原生元素。
import "../styles/settings-kit.css";
import "../styles/settings-page.css";
import "../styles/remote-install.css";

const KIND_LABEL: Record<RemotePackageKind, string> = {
  skill: "技能",
  plugin: "插件",
  pet: "宠物",
};

const KIND_TOPIC: Record<RemotePackageKind, string> = {
  skill: "mnemora-skill",
  plugin: "mnemora-plugin",
  pet: "mnemora-pet",
};

type Props = {
  kind: RemotePackageKind;
  /** 从对话里带过来的初始关键词。 */
  initialQuery?: string;
  onClose: () => void;
  /** 安装成功后回传结果消息，让调用方决定怎么呈现。 */
  onInstalled: (message: string) => void;
};

type Stage = "search" | "confirm";

/**
 * 从 GitHub 安装资源包的三步对话框：搜索 → 确认清单 → 安装。
 *
 * 「选哪个仓库」这个决定必须由人做出，因此候选列表只提供判断信号
 * （星数、最近更新、归档、许可证），不做自动挑选、不排名推荐。
 * 下载完成后先展示真实清单与权限声明，用户确认后才落盘安装。
 */
export function RemoteInstallDialog({ kind, initialQuery = "", onClose, onInstalled }: Props) {
  const [stage, setStage] = useState<Stage>("search");
  const [query, setQuery] = useState(initialQuery);
  const [result, setResult] = useState<RemoteSearchResult | null>(null);
  const [preview, setPreview] = useState<RemotePackagePreview | null>(null);
  const [busy, setBusy] = useState<"search" | "fetch" | "install" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    searchInputRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, onClose]);

  const runSearch = useCallback(async () => {
    if (!query.trim()) return;
    setBusy("search");
    setError(null);
    try {
      setResult(await searchRemotePackages(kind, query));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setResult(null);
    } finally {
      setBusy(null);
    }
  }, [kind, query]);

  // 初始关键词来自 Slash 命令参数，直接跑一次搜索省一步。
  useEffect(() => {
    if (initialQuery.trim()) void runSearch();
    // 只在挂载时自动搜一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const choose = async (candidate: RemoteCandidate) => {
    setBusy("fetch");
    setError(null);
    try {
      const fetched = await fetchRemotePackage(kind, candidate.fullName);
      setPreview(fetched);
      setAcknowledged(false);
      setStage("confirm");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const confirmInstall = async () => {
    if (!preview) return;
    setBusy("install");
    setError(null);
    try {
      const message = await installRemotePackage(preview.stagingToken, preview.replacesExisting);
      onInstalled(message);
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      // token 已被后端消费，必须退回搜索重新取，不能让用户对着失效 token 反复点。
      setPreview(null);
      setStage("search");
    } finally {
      setBusy(null);
    }
  };

  const blocking = preview?.warnings.filter((item) => item.includes("会被拒绝")) ?? [];

  return (
    <div className="remote-install-backdrop" role="presentation" onClick={() => !busy && onClose()}>
      <section
        className="remote-install-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-install-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="remote-install-header">
          <div>
            <h2 id="remote-install-title">从 GitHub 安装{KIND_LABEL[kind]}</h2>
            <span>
              {stage === "search"
                ? `按关键词搜索带 ${KIND_TOPIC[kind]} 标签的仓库，由你决定安装哪一个`
                : "确认清单内容与权限声明后再安装"}
            </span>
          </div>
          <button className="remote-install-close" type="button" aria-label="关闭" disabled={busy !== null} onClick={onClose}>
            <X size={17} />
          </button>
        </header>

        <div className="remote-install-body">
          {error ? (
            <div className="settings-callout settings-callout-danger" role="alert">
              <AlertTriangle size={17} />
              <span>{error}</span>
            </div>
          ) : null}

          {stage === "search" ? (
            <>
              <form
                className="remote-install-search"
                onSubmit={(event) => {
                  event.preventDefault();
                  void runSearch();
                }}
              >
                <input
                  ref={searchInputRef}
                  className="settings-input"
                  value={query}
                  placeholder={`搜索${KIND_LABEL[kind]}关键词，例如 weather`}
                  aria-label="搜索关键词"
                  onChange={(event) => setQuery(event.target.value)}
                />
                <button className="settings-button settings-button-primary" type="submit" disabled={busy !== null || !query.trim()}>
                  {busy === "search" ? <LoaderCircle className="settings-spin" size={15} /> : <Search size={15} />}
                  <span>搜索</span>
                </button>
              </form>

              <div className="settings-callout settings-callout-warning">
                <ShieldAlert size={17} />
                <div>
                  <strong>安装前请自行确认来源</strong>
                  <span>
                    搜索结果直接来自 GitHub，未经审核。{KIND_LABEL[kind]}
                    包可能包含会进入模型上下文的说明文本，请优先选择你认识的作者与活跃维护的仓库。
                  </span>
                </div>
              </div>

              {busy === "fetch" ? (
                <div className="settings-loading"><LoaderCircle className="settings-spin" size={20} />正在下载并解析仓库快照</div>
              ) : result ? (
                <>
                  {result.candidates.length === 0 ? (
                    <div className="settings-empty">
                      <Search size={26} />
                      <strong>没有找到匹配的仓库</strong>
                      <span>请确认仓库已加上 {KIND_TOPIC[kind]} 标签，或换一个关键词。</span>
                    </div>
                  ) : (
                    <>
                      <p className="settings-section-note">
                        共 {result.totalCount} 个结果{result.truncated ? "，只显示前 20 个；关键词越具体越准确" : ""}
                      </p>
                      <ul className="remote-candidate-list">
                        {result.candidates.map((candidate) => (
                          <li key={candidate.fullName}>
                            <div className="remote-candidate-main">
                              <div className="remote-candidate-title">
                                <strong>{candidate.fullName}</strong>
                                {candidate.archived ? <span className="settings-pill settings-pill-warning">已归档</span> : null}
                                {candidate.license ? <span className="settings-pill">{candidate.license}</span> : null}
                              </div>
                              <p>{candidate.description || "仓库没有填写描述。"}</p>
                              <div className="remote-candidate-meta">
                                <span><Star size={12} />{candidate.stars}</span>
                                <span>更新于 {formatRelativeDate(candidate.pushedAt)}</span>
                                <span>{candidate.owner}</span>
                              </div>
                            </div>
                            <div className="remote-candidate-actions">
                              <button
                                className="settings-button settings-button-secondary"
                                type="button"
                                title="在浏览器中查看仓库"
                                onClick={() => window.open(candidate.htmlUrl, "_blank", "noopener,noreferrer")}
                              >
                                <ExternalLink size={14} /><span>查看</span>
                              </button>
                              <button
                                className="settings-button settings-button-primary"
                                type="button"
                                disabled={busy !== null}
                                onClick={() => void choose(candidate)}
                              >
                                <ArchiveRestore size={14} /><span>取回并检查</span>
                              </button>
                            </div>
                          </li>
                        ))}
                      </ul>
                    </>
                  )}
                </>
              ) : null}
            </>
          ) : preview ? (
            <RemotePreviewPanel
              preview={preview}
              blocking={blocking}
              acknowledged={acknowledged}
              busy={busy}
              onAcknowledge={setAcknowledged}
              onBack={() => {
                setPreview(null);
                setStage("search");
              }}
              onConfirm={() => void confirmInstall()}
            />
          ) : null}
        </div>
      </section>
    </div>
  );
}

function RemotePreviewPanel({
  preview,
  blocking,
  acknowledged,
  busy,
  onAcknowledge,
  onBack,
  onConfirm,
}: {
  preview: RemotePackagePreview;
  blocking: string[];
  acknowledged: boolean;
  busy: "search" | "fetch" | "install" | null;
  onAcknowledge: (value: boolean) => void;
  onBack: () => void;
  onConfirm: () => void;
}) {
  return (
    <>
      <div className="settings-stat-grid remote-preview-stats">
        <div className="settings-stat"><span>名称</span><strong title={preview.name}>{preview.name || "未声明"}</strong></div>
        <div className="settings-stat"><span>版本</span><strong>{preview.version || "未声明"}</strong></div>
        <div className="settings-stat"><span>发布者</span><strong title={preview.publisher}>{preview.publisher || "未声明"}</strong></div>
        <div className="settings-stat"><span>内容</span><strong>{preview.fileCount} 个文件 · {formatBytes(preview.totalBytes)}</strong></div>
      </div>

      <dl className="remote-preview-source">
        <div><dt>来源仓库</dt><dd>{preview.fullName}</dd></div>
        <div><dt>提交</dt><dd>{preview.commitSha.slice(0, 7) || "未知"}</dd></div>
        {preview.id ? <div><dt>标识</dt><dd>{preview.id}</dd></div> : null}
      </dl>

      {preview.description ? <p className="settings-section-note">{preview.description}</p> : null}

      {preview.kind === "plugin" ? (
        <div className="remote-preview-caps">
          <div>
            <span>贡献 Skill</span>
            <strong>{preview.skillCount} 个</strong>
          </div>
          <div>
            <span>声明 MCP 服务器</span>
            <strong>{preview.mcpServerIds.length > 0 ? preview.mcpServerIds.join("、") : "无"}</strong>
          </div>
          <div>
            <span>网络域名</span>
            <strong>{preview.networkDomains.length > 0 ? preview.networkDomains.join("、") : "无"}</strong>
          </div>
          <div>
            <span>凭据权限</span>
            <strong>{preview.secrets.length > 0 ? preview.secrets.join("、") : "无"}</strong>
          </div>
        </div>
      ) : null}

      {preview.warnings.map((warning) => (
        <div className={`settings-callout ${warning.includes("会被拒绝") ? "settings-callout-danger" : "settings-callout-warning"}`} key={warning}>
          <AlertTriangle size={17} />
          <span>{warning}</span>
        </div>
      ))}

      {preview.replacesExisting ? (
        <div className="settings-callout settings-callout-warning">
          <AlertTriangle size={17} />
          <div>
            <strong>将覆盖已安装的同名条目</strong>
            <span>标识 {preview.id} 已存在。继续安装会用这个版本替换它。</span>
          </div>
        </div>
      ) : null}

      {/* 明确勾选而不是只有一个「安装」按钮：这里要的是知情，不是点得快。 */}
      <label className="settings-check remote-preview-ack">
        <input
          type="checkbox"
          checked={acknowledged}
          disabled={blocking.length > 0}
          onChange={(event) => onAcknowledge(event.target.checked)}
        />
        <span>
          <strong>我确认信任这个来源</strong>
          <small>
            签名验证尚未接入可信发布者目录，因此这个包会被视为未验证内容。
            {preview.kind === "plugin" ? "插件安装后保持停用，需要你在插件设置中手动启用。" : null}
            {preview.kind === "skill" ? "技能说明文本会在被激活时进入模型上下文。" : null}
          </small>
        </span>
      </label>

      <footer className="remote-install-footer">
        <button className="settings-button settings-button-secondary" type="button" disabled={busy !== null} onClick={onBack}>
          返回候选列表
        </button>
        <button
          className="settings-button settings-button-primary"
          type="button"
          disabled={busy !== null || !acknowledged || blocking.length > 0}
          title={blocking.length > 0 ? "该包声明了会被安装器拒绝的能力" : undefined}
          onClick={onConfirm}
        >
          {busy === "install" ? <LoaderCircle className="settings-spin" size={15} /> : <ArchiveRestore size={15} />}
          <span>{preview.replacesExisting ? "替换安装" : "确认安装"}</span>
        </button>
      </footer>
    </>
  );
}

export default RemoteInstallDialog;
