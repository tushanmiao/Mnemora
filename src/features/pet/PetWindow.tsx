import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { loadApplicationSettings } from "../settings/api/appSettings";
import type { PetSettings } from "../../types/appSettings";
import { openMainFromPet, setPetEnabled, updatePetPosition } from "./api";
import { PetMascot } from "./PetMascot";
import type { PetStatePayload } from "./types";
import "./pet.css";

const idleState: PetStatePayload = {
  state: "idle",
  label: "陪你学习",
  detail: "需要时我会告诉你进度",
  updatedAt: Date.now(),
};

export default function PetWindow() {
  const [settings, setSettings] = useState<PetSettings | null>(null);
  const [state, setState] = useState(idleState);
  const positionTimer = useRef<number | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlistenState: (() => void) | undefined;
    let unlistenSettings: (() => void) | undefined;
    let unlistenMoved: (() => void) | undefined;
    void loadApplicationSettings().then((value) => {
      if (!disposed) setSettings(value.pet);
    });
    void listen<PetStatePayload>("mnemora://pet-state", (event) => {
      if (!disposed) setState(event.payload);
    }).then((unlisten) => { unlistenState = unlisten; });
    void listen<PetSettings>("mnemora://pet-settings", (event) => {
      if (!disposed) setSettings(event.payload);
    }).then((unlisten) => { unlistenSettings = unlisten; });
    const current = getCurrentWindow();
    void current.onMoved(({ payload }) => {
      if (positionTimer.current !== null) window.clearTimeout(positionTimer.current);
      positionTimer.current = window.setTimeout(async () => {
        const scale = await current.scaleFactor();
        const logical = payload.toLogical(scale);
        await updatePetPosition(logical.x, logical.y);
      }, 320);
    }).then((unlisten) => { unlistenMoved = unlisten; });
    void emitTo("main", "mnemora://pet-ready");
    return () => {
      disposed = true;
      unlistenState?.();
      unlistenSettings?.();
      unlistenMoved?.();
      if (positionTimer.current !== null) window.clearTimeout(positionTimer.current);
    };
  }, []);

  if (!settings) return <main className="pet-window-shell pet-window-loading" />;

  const beginDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || settings.clickThrough) return;
    if ((event.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging();
  };

  return (
    <main
      className="pet-window-shell"
      data-state={state.state}
      style={{
        opacity: settings.opacity / 100,
        "--pet-size": String(settings.size) + "px",
      } as CSSProperties}
      onPointerDown={beginDrag}
    >
      {settings.speechBubbles ? (
        <button className="pet-bubble" type="button" onClick={() => void openMainFromPet()}>
          <strong>{state.label}</strong>
          <span>{state.detail}</span>
        </button>
      ) : null}
      <div
        className="pet-character-button"
        title="打开 Mnemora"
        role="img"
        aria-label="Mnemora 桌面宠物；拖动可移动，双击打开应用"
        onDoubleClick={() => void openMainFromPet()}
      >
        <PetMascot state={state.state} reducedMotion={settings.reducedMotion} />
      </div>
      {!settings.clickThrough ? (
        <button
          className="pet-close"
          type="button"
          title="关闭桌面宠物"
          onClick={() => void setPetEnabled(false)}
        >
          ×
        </button>
      ) : null}
    </main>
  );
}
