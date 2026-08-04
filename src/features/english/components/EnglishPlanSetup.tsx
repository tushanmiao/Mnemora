import { BookPlus, Check, Upload } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { EnglishGroupSummary } from "../api/english";
import {
  defaultEnglishPlanSettings,
  type EnglishPlanSettings,
} from "../api/learning";

type Props = {
  groups: EnglishGroupSummary[];
  busy: boolean;
  onCreate: (name: string, groupIds: number[], settings: EnglishPlanSettings) => Promise<void>;
  onImport: () => Promise<void>;
};

export default function EnglishPlanSetup({ groups, busy, onCreate, onImport }: Props) {
  const [name, setName] = useState("雅思核心词书");
  const [selectedGroups, setSelectedGroups] = useState<number[]>([]);
  const [settings, setSettings] = useState(defaultEnglishPlanSettings);

  useEffect(() => {
    if (selectedGroups.length === 0 && groups.length > 0) {
      setSelectedGroups(groups.map((group) => group.id));
    }
  }, [groups, selectedGroups.length]);

  const itemCount = useMemo(
    () => groups.filter((group) => selectedGroups.includes(group.id)).reduce((sum, group) => sum + group.count, 0),
    [groups, selectedGroups],
  );

  const toggleGroup = (id: number) => {
    setSelectedGroups((current) => current.includes(id)
      ? current.filter((groupId) => groupId !== id)
      : [...current, id]);
  };

  return (
    <section className="english-plan-setup" aria-labelledby="english-plan-title">
      <div className="english-section-heading">
        <div>
          <h2 id="english-plan-title">建立学习计划</h2>
          <p>选择词典范围，并分别设置每组数量与每日目标。</p>
        </div>
        <BookPlus size={20} />
      </div>

      <div className="english-plan-form">
        <label className="english-field english-field-wide">
          <span>词书名称</span>
          <input value={name} maxLength={80} onChange={(event) => setName(event.target.value)} />
        </label>
        <NumberField label="每组新词数" value={settings.newBatchSize} min={1} max={100} onChange={(value) => setSettings({ ...settings, newBatchSize: value })} />
        <NumberField label="每日新词目标" value={settings.dailyNewTarget} min={1} max={500} onChange={(value) => setSettings({ ...settings, dailyNewTarget: value })} />
        <NumberField label="每组复习数" value={settings.reviewBatchSize} min={1} max={100} onChange={(value) => setSettings({ ...settings, reviewBatchSize: value })} />
        <NumberField label="每日复习软目标" value={settings.dailyReviewTarget} min={1} max={2000} onChange={(value) => setSettings({ ...settings, dailyReviewTarget: value })} />
        <label className="english-field">
          <span>复习强度</span>
          <select value={settings.desiredRetention} onChange={(event) => setSettings({ ...settings, desiredRetention: Number(event.target.value) })}>
            <option value={0.85}>轻量 · 85%</option>
            <option value={0.9}>标准 · 90%</option>
            <option value={0.95}>强化 · 95%</option>
          </select>
        </label>
        <label className="english-field">
          <span>首选发音</span>
          <select value={settings.preferredAccent} onChange={(event) => setSettings({ ...settings, preferredAccent: event.target.value as "british" | "american" })}>
            <option value="british">英音</option>
            <option value="american">美音</option>
          </select>
        </label>
      </div>

      <div className="english-group-picker">
        <div className="english-group-picker-heading">
          <span>词典分组</span>
          <button type="button" onClick={() => setSelectedGroups(selectedGroups.length === groups.length ? [] : groups.map((group) => group.id))}>
            {selectedGroups.length === groups.length ? "取消全选" : "选择全部"}
          </button>
        </div>
        <div className="english-group-options">
          {groups.map((group) => {
            const selected = selectedGroups.includes(group.id);
            return (
              <label key={group.id} className={selected ? "is-selected" : ""}>
                <input type="checkbox" checked={selected} onChange={() => toggleGroup(group.id)} />
                <span><Check size={14} />{group.name}</span>
                <small>{group.count.toLocaleString()}</small>
              </label>
            );
          })}
        </div>
      </div>

      <div className="english-plan-submit">
        <span>{selectedGroups.length} 个分组 · {itemCount.toLocaleString()} 个单词</span>
        <div className="english-plan-submit-actions">
          <button className="english-secondary-button" type="button" disabled={busy} onClick={() => void onImport()}><Upload size={16} />导入自定义词书</button>
          <button type="button" disabled={busy || selectedGroups.length === 0 || !name.trim()} onClick={() => void onCreate(name, selectedGroups, settings)}>
            <BookPlus size={16} />{busy ? "正在建立计划" : "创建主计划"}
          </button>
        </div>
      </div>
    </section>
  );
}

function NumberField({ label, value, min, max, onChange }: { label: string; value: number; min: number; max: number; onChange: (value: number) => void }) {
  return (
    <label className="english-field">
      <span>{label}</span>
      <input type="number" value={value} min={min} max={max} onChange={(event) => onChange(Math.min(max, Math.max(min, Number(event.target.value) || min)))} />
    </label>
  );
}
