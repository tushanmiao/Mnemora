import { useEffect, useMemo, useState, type CSSProperties } from "react";
import type { PetDescriptor, PetState } from "./types";

const ROWS: Record<PetState, { row: number; durations: number[] }> = {
  idle: { row: 0, durations: [280, 110, 110, 140, 140, 320] },
  thinking: { row: 7, durations: [150, 150, 150, 150, 150, 220] },
  tooling: { row: 8, durations: [150, 150, 150, 150, 150, 280] },
  waiting: { row: 6, durations: [150, 150, 150, 150, 150, 260] },
  success: { row: 3, durations: [140, 140, 140, 280] },
  error: { row: 5, durations: [140, 140, 140, 140, 140, 140, 140, 240] },
};

export function PetSprite({
  pet,
  state,
  reducedMotion,
}: {
  pet: PetDescriptor;
  state: PetState;
  reducedMotion: boolean;
}) {
  const animation = ROWS[state];
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    setFrame(0);
    if (reducedMotion || animation.durations.length <= 1) return undefined;
    let cancelled = false;
    let timer: number | undefined;
    const schedule = (current: number) => {
      timer = window.setTimeout(() => {
        if (cancelled) return;
        const next = (current + 1) % animation.durations.length;
        setFrame(next);
        schedule(next);
      }, animation.durations[current]);
    };
    schedule(0);
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [animation.durations, reducedMotion, state]);

  const columns = pet.columns ?? 8;
  const rows = pet.rows ?? 9;
  const style = useMemo(() => ({
    width: `${columns * 100}%`,
    height: `${rows * 100}%`,
    transform: `translate(${-frame * (100 / columns)}%, ${-animation.row * (100 / rows)}%)`,
  } as CSSProperties), [animation.row, columns, frame, rows]);

  return (
    <div
      className="pet-sprite"
      data-reduced-motion={reducedMotion ? "true" : "false"}
      aria-hidden="true"
    >
      <img
        className="pet-sprite-atlas"
        src={pet.spritesheetUrl ?? ""}
        style={style}
        alt=""
        draggable={false}
      />
    </div>
  );
}
