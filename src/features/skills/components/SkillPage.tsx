import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  ExternalLink,
  FileArchive,
  FolderInput,
  LoaderCircle,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import type { SkillDetail, SkillImportKind, SkillSource, SkillSummary } from "../../../types/skill";
import { getSkillDetail } from "../api/skills";
import type { useSkills } from "../hooks/useSkills";
import "../styles/skills.css";

type SkillState = ReturnType<typeof useSkills>;

type Props = {
  state: SkillState;
};

type Filter = "all" | SkillSource | "disabled";

export function SkillManager({ state }: Props) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState("");

  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase("zh-CN");
    return state.skills.filter((skill) => {
      if (filter === "disabled" && skill.enabled) return false;
      if (filter === "builtin" && skill.source !== "builtin") return false;
      if (filter === "user" && skill.source !== "user") return false;
      return !normalized || [skill.name, skill.id, skill.description]
        .some((value) => value.toLocaleLowerCase("zh-CN").includes(normalized));
    });
  }, [filter, query, state.skills]);

  useEffect(() => {
    if (!detail) return;
    const current = state.skills.find((skill) => skill.id === detail.id);
    if (!current) setDetail(null);
    else if (current.enabled !== detail.enabled) setDetail((value) => value ? { ...value, enabled: current.enabled } : null);
  }, [detail, state.skills]);

  const showDetail = async (skill: SkillSummary) => {
    setDetailLoading(true);
    setDetailError("");
    try {
      setDetail(await getSkillDetail(skill.id));
    } catch (reason) {
      setDetailError(errorMessage(reason, "读取技能详情失败。"));
    } finally {
      setDetailLoading(false);
    }
  };

  const chooseImport = async (kind: SkillImportKind) => {
    const selected = await open(kind === "directory"
      ? { directory: true, multiple: false, title: "选择包含 SKILL.md 的目录" }
      : { directory: false, multiple: false, title: "选择 Skill ZIP", filters: [{ name: "Skill ZIP", extensions: ["zip"] }] });
    if (!selected || Array.isArray(selected)) return;
    const result = await state.install(selected, kind, false);
    if (result?.status !== "alreadyExists") return;
    if (window.confirm(`技能“${result.skill.name}”已经安装。是否用所选版本替换？`)) {
      await state.install(selected, kind, true);
    }
  };

  const openSourceRepository = async (url: string) => {
    try {
      await openUrl(url);
    } catch (reason) {
      setDetailError(errorMessage(reason, "无法打开上游仓库。"));
    }
  };

  return (
    <section className="settings-content skills-page" aria-label="技能设置">
      <header className="settings-content-heading skills-header">
        <div>
          <h1>技能</h1>
          <span>管理模型完成任务时可以采用的工作说明</span>
        </div>
        <div className="skills-header-actions">
          <button type="button" onClick={() => void chooseImport("directory")} disabled={state.busySkillId !== null}>
            <FolderInput size={16} /><span>导入目录</span>
          </button>
          <button type="button" onClick={() => void chooseImport("zip")} disabled={state.busySkillId !== null}>
            <FileArchive size={16} /><span>导入 ZIP</span>
          </button>
          <button className="icon-button" type="button" title="刷新技能" aria-label="刷新技能" disabled={state.loading} onClick={() => void state.refresh()}>
            <RefreshCw size={17} />
          </button>
        </div>
      </header>

      <div className="skills-toolbar">
        <label className="skills-search">
          <Search size={16} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索技能" />
        </label>
        <div className="skills-filters" aria-label="技能筛选">
          {([
            ["all", "全部"],
            ["builtin", "内置"],
            ["user", "用户安装"],
            ["disabled", "已禁用"],
          ] as const).map(([value, label]) => (
            <button className={filter === value ? "skills-filter-active" : ""} type="button" key={value} onClick={() => setFilter(value)}>{label}</button>
          ))}
        </div>
      </div>

      {state.error ? <div className="skills-feedback skills-feedback-error"><AlertCircle size={16} /><span>{state.error}</span></div> : null}
      {state.warnings.length > 0 ? <div className="skills-feedback"><AlertCircle size={16} /><span>{state.warnings.join("；")}</span></div> : null}

      <div className="skills-body">
        <div className="skills-list" aria-busy={state.loading}>
          {state.loading ? <div className="skills-empty"><LoaderCircle className="skills-spin" size={20} />正在读取技能</div> : null}
          {!state.loading && filtered.length === 0 ? <div className="skills-empty">没有符合条件的技能</div> : null}
          {filtered.map((skill) => (
            <article className={`skill-row${skill.enabled ? "" : " skill-row-disabled"}`} key={skill.id}>
              <button className="skill-row-main" type="button" onClick={() => void showDetail(skill)}>
                <span className="skill-source">
                  {skill.source === "builtin"
                    ? skill.provenance.repository ? "开源适配" : "内置"
                    : "用户"}
                </span>
                <strong>{skill.name}</strong>
                <small>{skill.description}</small>
                <span className="skill-meta">{skill.id} · v{skill.version}{skill.triggers.length ? ` · ${skill.triggers.join(" ")}` : ""}</span>
              </button>
              <div className="skill-row-actions">
                <label className="skill-switch" title={skill.enabled ? "禁用技能" : "启用技能"}>
                  <input
                    type="checkbox"
                    checked={skill.enabled}
                    disabled={state.busySkillId !== null}
                    onChange={(event) => void state.toggle(skill.id, event.target.checked)}
                  />
                  <span />
                </label>
                {skill.source === "user" ? (
                  <button
                    className="icon-button skill-delete"
                    type="button"
                    title="删除技能"
                    aria-label={`删除技能 ${skill.name}`}
                    disabled={state.busySkillId !== null}
                    onClick={() => {
                      if (window.confirm(`确定永久删除用户技能“${skill.name}”吗？原始导入目录不会被删除。`)) {
                        void state.uninstall(skill.id);
                      }
                    }}
                  >
                    <Trash2 size={16} />
                  </button>
                ) : !skill.enabled ? (
                  <button className="skill-restore" type="button" disabled={state.busySkillId !== null} onClick={() => void state.restore(skill.id)}>恢复</button>
                ) : null}
              </div>
            </article>
          ))}
        </div>

        {detail || detailLoading || detailError ? (
          <aside className="skill-detail" aria-label="技能详情">
            <button className="icon-button skill-detail-close" type="button" title="关闭详情" aria-label="关闭详情" onClick={() => { setDetail(null); setDetailError(""); }}>
              <X size={17} />
            </button>
            {detailLoading ? <div className="skills-empty"><LoaderCircle className="skills-spin" size={20} />正在读取详情</div> : null}
            {detailError ? <div className="skills-feedback skills-feedback-error"><AlertCircle size={16} />{detailError}</div> : null}
            {detail && !detailLoading ? (
              <>
                <span className="skill-source">{detail.source === "builtin" ? "内置技能" : "用户技能"}</span>
                <h2>{detail.name}</h2>
                <p>{detail.description}</p>
                <dl>
                  <div><dt>ID</dt><dd>{detail.id}</dd></div>
                  <div><dt>版本</dt><dd>{detail.version}</dd></div>
                  <div><dt>许可证</dt><dd>{detail.license || "未声明"}</dd></div>
                  <div><dt>内容哈希</dt><dd title={detail.contentHash}>{detail.contentHash.slice(0, 22)}...</dd></div>
                  <div><dt>建议工具</dt><dd>{detail.recommendedTools.join("、") || "无"}</dd></div>
                  <div><dt>必需工具</dt><dd>{detail.requiredTools.join("、") || "无"}</dd></div>
                </dl>
                {detail.provenance.repository ? (
                  <section className="skill-provenance" aria-label="上游来源">
                    <div className="skill-provenance-heading">
                      <h3>上游来源</h3>
                      <button type="button" onClick={() => void openSourceRepository(detail.provenance.repository!)}>
                        <ExternalLink size={14} />
                        <span>打开仓库</span>
                      </button>
                    </div>
                    <dl>
                      <div><dt>仓库</dt><dd title={detail.provenance.repository}>{repositoryName(detail.provenance.repository)}</dd></div>
                      <div><dt>原始路径</dt><dd title={detail.provenance.path}>{detail.provenance.path || "未记录"}</dd></div>
                      <div><dt>固定版本</dt><dd title={detail.provenance.revision}>{shortRevision(detail.provenance.revision)}</dd></div>
                      <div><dt>适配状态</dt><dd>{detail.provenance.adapted ? "已为 Mnemora 适配" : "原样导入"}</dd></div>
                    </dl>
                    {detail.provenance.attribution ? <p><strong>署名</strong>{detail.provenance.attribution}</p> : null}
                    {detail.provenance.adaptationNotes ? <p><strong>改编说明</strong>{detail.provenance.adaptationNotes}</p> : null}
                    {detail.compatibility ? <p><strong>能力边界</strong>{detail.compatibility}</p> : null}
                  </section>
                ) : detail.compatibility ? (
                  <section className="skill-provenance" aria-label="兼容性说明">
                    <p><strong>能力边界</strong>{detail.compatibility}</p>
                  </section>
                ) : null}
                <h3>SKILL.md</h3>
                <pre>{detail.markdown}</pre>
                <h3>文件</h3>
                <ul>{detail.files.map((file) => <li key={file.path}><span>{file.path}</span><small>{formatBytes(file.sizeBytes)}</small></li>)}</ul>
              </>
            ) : null}
          </aside>
        ) : null}
      </div>
    </section>
  );
}

function errorMessage(reason: unknown, fallback: string) {
  if (reason instanceof Error) return reason.message;
  return typeof reason === "string" ? reason : fallback;
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function repositoryName(value: string) {
  try {
    return new URL(value).pathname.replace(/^\//, "") || value;
  } catch {
    return value;
  }
}

function shortRevision(value?: string) {
  if (!value) return "未记录";
  return value.length > 12 ? `${value.slice(0, 12)}...` : value;
}
