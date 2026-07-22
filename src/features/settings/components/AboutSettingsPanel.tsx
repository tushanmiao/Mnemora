export function AboutSettingsPanel() {
  return (
    <section className="settings-content" aria-label="关于 Mnemora">
      <div className="settings-content-heading">
        <div>
          <h2>关于</h2>
          <span>Mnemora 桌面 AI 阅读与对话工具</span>
        </div>
      </div>
      <div className="settings-about">
        <strong>Mnemora</strong>
        <span>版本 0.1.0</span>
        <p>基于 Tauri 2、Rust、React 和 TypeScript 构建。</p>
      </div>
    </section>
  );
}
