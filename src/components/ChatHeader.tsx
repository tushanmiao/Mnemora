import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, Moon, MoreHorizontal, PanelRight, ShieldCheck, Sun } from "lucide-react";
import { AI_PERMISSION_LABELS, type AiPermissionMode } from "../types/chat";
import "../styles/chat-header.css";

const permissionOptions = Object.keys(AI_PERMISSION_LABELS) as AiPermissionMode[];

type ChatHeaderProps = {
  title: string;
  permission: AiPermissionMode;
  permissionDisabled: boolean;
  theme: "light" | "dark";
  onPermissionChange: (permission: AiPermissionMode) => void;
  onToggleTheme: () => void;
};

export function ChatHeader({
  title,
  permission,
  permissionDisabled,
  theme,
  onPermissionChange,
  onToggleTheme,
}: ChatHeaderProps) {
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
        <h1>{title}</h1>
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
            disabled={permissionDisabled}
            onClick={() => setPermissionMenuOpen((open) => !open)}
          >
            <ShieldCheck size={17} />
            <span>{AI_PERMISSION_LABELS[permission]}</span>
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
                    onPermissionChange(option);
                    setPermissionMenuOpen(false);
                  }}
                >
                  <span>{AI_PERMISSION_LABELS[option]}</span>
                  {permission === option ? <Check size={16} /> : null}
                </button>
              ))}
            </div>
          ) : null}
        </div>
        <button
          className="icon-button"
          type="button"
          title={theme === "light" ? "切换到深色模式" : "切换到浅色模式"}
          onClick={onToggleTheme}
        >
          {theme === "light" ? <Moon size={18} /> : <Sun size={18} />}
        </button>
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
