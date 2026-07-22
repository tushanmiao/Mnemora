import { getName, getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  BookOpen,
  CheckCircle2,
  Cpu,
  Database,
  ExternalLink,
  FileText,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import { useEffect, useState } from "react";
import "../styles/about-settings.css";

type AppMetadata = {
  name: string;
  version: string;
  tauriVersion: string;
};

const FALLBACK_METADATA: AppMetadata = {
  name: "Mnemora",
  version: "开发环境",
  tauriVersion: "不可用",
};

const PROJECT_URL = "https://github.com/tushanmiao/Mnemora";
const RELEASE_URL = PROJECT_URL + "/releases";
const ISSUES_URL = PROJECT_URL + "/issues";

export function AboutSettingsPanel() {
  const [metadata, setMetadata] = useState<AppMetadata>(FALLBACK_METADATA);
  const [metadataLoading, setMetadataLoading] = useState(true);
  const [metadataError, setMetadataError] = useState(false);

  useEffect(() => {
    let active = true;

    if (!isTauri()) {
      setMetadataLoading(false);
      return () => {
        active = false;
      };
    }

    void Promise.all([getName(), getVersion(), getTauriVersion()])
      .then(([name, version, tauriVersion]) => {
        if (!active) return;
        setMetadata({ name, version, tauriVersion });
      })
      .catch(() => {
        if (active) setMetadataError(true);
      })
      .finally(() => {
        if (active) setMetadataLoading(false);
      });

    return () => {
      active = false;
    };
  }, []);

  const openExternal = async (url: string) => {
    try {
      if (isTauri()) {
        await openUrl(url);
      } else {
        window.open(url, "_blank", "noopener,noreferrer");
      }
    } catch {
      setMetadataError(true);
    }
  };

  const desktopCore = metadata.tauriVersion === "不可用"
    ? "Tauri 2"
    : "Tauri " + metadata.tauriVersion;

  return (
    <section className="settings-content about-settings-content" aria-label="关于 Mnemora">
      <div className="settings-content-heading">
        <div>
          <h2>关于</h2>
          <span>项目状态、运行环境和安全边界</span>
        </div>
        <div className="about-version-mark">
          <CheckCircle2 size={15} />
          <span>测试版</span>
        </div>
      </div>

      <div className="about-settings-scroll">
        <section className="about-hero" aria-labelledby="about-product-name">
          <div className="about-hero-icon" aria-hidden="true">
            <Sparkles size={25} />
          </div>
          <div className="about-hero-copy">
            <h3 id="about-product-name">{metadata.name}</h3>
            <p>轻量、流畅、可扩展的桌面 AI 对话与阅读工作台。</p>
            <span>
              使用 Tauri 2、Rust、React 和 TypeScript 构建，重点优化多模型接入、流式体验和本地数据边界。
            </span>
          </div>
        </section>

        <section className="about-settings-section" aria-labelledby="about-runtime-heading">
          <div className="about-section-heading">
            <Cpu size={16} />
            <h3 id="about-runtime-heading">运行环境</h3>
          </div>
          <div className="about-metadata-grid">
            <MetadataItem label="应用版本" value={metadata.version} loading={metadataLoading} />
            <MetadataItem label="桌面核心" value={desktopCore} loading={metadataLoading} />
            <MetadataItem label="前端运行时" value="React 19 · TypeScript 5.8" />
            <MetadataItem label="模型协议" value="OpenAI · Anthropic · Gemini" />
          </div>
          {metadataError ? (
            <p className="about-inline-warning">部分运行环境信息读取失败，当前显示的是可用的备用信息。</p>
          ) : null}
        </section>

        <section className="about-settings-section" aria-labelledby="about-capability-heading">
          <div className="about-section-heading">
            <BookOpen size={16} />
            <h3 id="about-capability-heading">当前能力</h3>
          </div>
          <div className="about-capability-list">
            <CapabilityItem
              title="多供应商对话"
              description="支持四种模型协议、自定义中转站、Display Name 映射和手动连接测试。"
            />
            <CapabilityItem
              title="流式与长对话"
              description="流式思考、块级 Markdown、上下文用量提示和接近 90% 时的自动压缩。"
            />
            <CapabilityItem
              title="Skill 与记忆"
              description="支持来源可追溯的 Skill、L1/L2 记忆和受权限控制的工具循环。"
            />
            <CapabilityItem
              title="附件与安全预览"
              description="支持常见文本、图片、PDF、DOCX、XLSX 附件及受限 HTML 预览。"
            />
          </div>
        </section>

        <section className="about-settings-section" aria-labelledby="about-security-heading">
          <div className="about-section-heading">
            <ShieldCheck size={16} />
            <h3 id="about-security-heading">数据与安全</h3>
          </div>
          <div className="about-fact-list">
            <FactItem
              icon={<Database size={15} />}
              title="本地优先"
              text="会话、记忆和用量保存在本地文件；不使用远程同步数据库。"
            />
            <FactItem
              icon={<ShieldCheck size={15} />}
              title="凭据隔离"
              text="API Key 使用系统凭据存储，普通设置读取只返回凭据是否存在。"
            />
            <FactItem
              icon={<FileText size={15} />}
              title="预览受限"
              text="HTML 预览经过清洗和 CSP 限制，关闭窗口后立即销毁临时内容。"
            />
          </div>
        </section>

        <section className="about-settings-section about-roadmap-section" aria-labelledby="about-roadmap-heading">
          <div className="about-section-heading">
            <Sparkles size={16} />
            <h3 id="about-roadmap-heading">后续方向</h3>
          </div>
          <p className="about-section-description">
            PDF 阅读、Zotero 类笔记与批注、Office 受控工具和对话导出正在规划中。当前 Agent 不提供任意 Shell 或桌面自动化权限。
          </p>
        </section>

        <div className="about-link-row" aria-label="项目链接">
          <button type="button" className="settings-button settings-button-secondary" onClick={() => void openExternal(PROJECT_URL)}>
            <ExternalLink size={15} />
            <span>项目主页</span>
          </button>
          <button type="button" className="settings-button settings-button-secondary" onClick={() => void openExternal(RELEASE_URL)}>
            <ExternalLink size={15} />
            <span>下载与版本</span>
          </button>
          <button type="button" className="settings-button settings-button-secondary" onClick={() => void openExternal(ISSUES_URL)}>
            <ExternalLink size={15} />
            <span>反馈问题</span>
          </button>
        </div>

        <p className="about-footer-note">
          Mnemora 当前为测试版本。导出的设置备份可能包含 API Key，请勿公开分享。
        </p>
      </div>
    </section>
  );
}

function MetadataItem({
  label,
  value,
  loading = false,
}: {
  label: string;
  value: string;
  loading?: boolean;
}) {
  return (
    <div className="about-metadata-item">
      <span>{label}</span>
      <strong>{loading ? "读取中..." : value}</strong>
    </div>
  );
}

function CapabilityItem({ title, description }: { title: string; description: string }) {
  return (
    <div className="about-capability-item">
      <CheckCircle2 size={15} />
      <div>
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
    </div>
  );
}

function FactItem({
  icon,
  title,
  text,
}: {
  icon: React.ReactNode;
  title: string;
  text: string;
}) {
  return (
    <div className="about-fact-item">
      <span className="about-fact-icon">{icon}</span>
      <div>
        <strong>{title}</strong>
        <span>{text}</span>
      </div>
    </div>
  );
}
