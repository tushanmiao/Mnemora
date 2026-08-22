import { useEffect, useState } from "react";

type EditableRangeControlProps = {
  value: number;
  min: number;
  max: number;
  step: number;
  ariaLabel: string;
  suffix?: string;
  fractionDigits?: number;
  onChange: (value: number) => void;
};

export function EditableRangeControl({
  value,
  min,
  max,
  step,
  ariaLabel,
  suffix = "",
  fractionDigits = 0,
  onChange,
}: EditableRangeControlProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(formatNumber(value, fractionDigits));

  useEffect(() => {
    if (!editing) setDraft(formatNumber(value, fractionDigits));
  }, [editing, fractionDigits, value]);

  const commit = () => {
    const parsed = Number(draft);
    if (Number.isFinite(parsed)) onChange(clampToStep(parsed, min, max, step));
    setEditing(false);
  };

  return (
    <div className="font-size-control editable-range-control">
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-label={ariaLabel}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      {editing ? (
        <label className="editable-range-value">
          <input
            type="number"
            min={min}
            max={max}
            step={step}
            value={draft}
            autoFocus
            aria-label={`${ariaLabel}数值`}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={commit}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
              if (event.key === "Escape") {
                setDraft(formatNumber(value, fractionDigits));
                setEditing(false);
              }
            }}
          />
          {suffix ? <span>{suffix}</span> : null}
        </label>
      ) : (
        <output
          title="双击直接输入数值"
          tabIndex={0}
          onDoubleClick={() => setEditing(true)}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") setEditing(true);
          }}
        >
          {formatNumber(value, fractionDigits)}{suffix ? ` ${suffix}` : ""}
        </output>
      )}
    </div>
  );
}

function clampToStep(value: number, min: number, max: number, step: number) {
  const clamped = Math.min(max, Math.max(min, value));
  const precision = Math.max(0, decimalPlaces(step));
  const snapped = min + Math.round((clamped - min) / step) * step;
  return Number(snapped.toFixed(precision));
}

function decimalPlaces(value: number) {
  const source = String(value);
  return source.includes(".") ? source.length - source.indexOf(".") - 1 : 0;
}

function formatNumber(value: number, fractionDigits: number) {
  return fractionDigits > 0 ? value.toFixed(fractionDigits) : String(Math.round(value));
}
