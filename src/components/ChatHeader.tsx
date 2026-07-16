import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, MoreHorizontal, PanelRight, ShieldCheck } from "lucide-react";
import "../styles/chat-header.css";

const permissionOptions = ["每次确认", "敏感确认", "完全访问"] as const;

export function ChatHeader() {
  const [permission, setPermission] = useState<(typeof permissionOptions)[number]>("敏感确认");
  const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
  const permissionMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function closeMenu(event: MouseEvent) {
      if (!permissionMenuRef.current?.contains(event.target as Node)) {
        setPermissionMenuOpen(false);
      }
    }

    document.addEventListener("mousedown", closeMenu);
    return () => document.removeEventListener("mousedown", closeMenu);
  }, []);

  return (
    <header className="chat-header">
      <div className="chat-heading">
        <h1>欢迎使用 Mnemora</h1>
        <span>今天</span>
      </div>

      <div className="chat-header-actions">
        <button className="model-button" type="button">
          <span className="model-status" aria-hidden="true" />
          <span>GPT-5</span>
          <ChevronDown size={15} />
        </button>
        <div className="permission-control" ref={permissionMenuRef}>
          <button
            className="permission-button"
            type="button"
            title="AI 权限"
            aria-expanded={permissionMenuOpen}
            onClick={() => setPermissionMenuOpen((open) => !open)}
          >
            <ShieldCheck size={17} />
            <span>{permission}</span>
            <ChevronDown size={14} />
          </button>

          {permissionMenuOpen ? (
            <div className="permission-menu" role="menu" aria-label="AI 权限">
              {permissionOptions.map((option) => (
                <button
                  className="permission-option"
                  type="button"
                  role="menuitemradio"
                  aria-checked={permission === option}
                  key={option}
                  onClick={() => {
                    setPermission(option);
                    setPermissionMenuOpen(false);
                  }}
                >
                  <span>{option}</span>
                  {permission === option ? <Check size={16} /> : null}
                </button>
              ))}
            </div>
          ) : null}
        </div>
        <button className="icon-button" type="button" title="打开详情面板">
          <PanelRight size={18} />
        </button>
        <button className="icon-button" type="button" title="更多操作">
          <MoreHorizontal size={19} />
        </button>
      </div>
    </header>
  );
}
