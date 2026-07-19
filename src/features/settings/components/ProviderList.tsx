import { Plus } from "lucide-react";
import type { ProviderConfig } from "../../../types/modelSettings";

type ProviderListProps = {
  providers: ProviderConfig[];
  selectedProviderId: string | null;
  isConfigured: (provider: ProviderConfig) => boolean;
  onAdd: () => void;
  onSelect: (providerId: string) => void;
};

export function ProviderList({
  providers,
  selectedProviderId,
  isConfigured,
  onAdd,
  onSelect,
}: ProviderListProps) {
  return (
    <aside className="provider-list" aria-label="供应商列表">
      <button className="provider-add-button" type="button" onClick={onAdd}>
        <Plus size={15} />
        <span>添加中转站</span>
      </button>

      <div className="provider-list-items">
        {providers.map((provider) => {
          const selected = provider.id === selectedProviderId;
          const configured = isConfigured(provider);
          return (
            <button
              className={`provider-list-item${selected ? " provider-list-item-active" : ""}`}
              type="button"
              key={provider.id}
              aria-pressed={selected}
              onClick={() => onSelect(provider.id)}
            >
              <span
                className={`provider-list-dot${provider.enabled && configured ? " provider-list-dot-configured" : ""}`}
                aria-hidden="true"
              />
              <span className="provider-list-copy">
                <strong>{provider.name || "未命名供应商"}</strong>
                <span>
                  {provider.enabled ? (configured ? "已配置" : "未配置") : "已停用"}
                  {` · ${provider.models.length} 个模型`}
                </span>
              </span>
            </button>
          );
        })}
      </div>
    </aside>
  );
}
