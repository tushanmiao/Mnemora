import type { PetState } from "./types";

export function PetMascot({
  state,
  reducedMotion,
}: {
  state: PetState;
  reducedMotion: boolean;
}) {
  return (
    <div
      className="pet-mascot"
      data-state={state}
      data-reduced-motion={reducedMotion ? "true" : "false"}
      aria-hidden="true"
    >
      <span className="pet-seed pet-seed-one" />
      <span className="pet-seed pet-seed-two" />
      <span className="pet-seed pet-seed-three" />
      <svg viewBox="0 0 180 170" role="presentation">
        <path className="pet-shadow" d="M37 145c15 15 93 16 108-2-8 22-97 26-108 2Z" />
        <path className="pet-tail" d="M131 116c29-5 35 21 13 29 9-12 0-20-16-17Z" />
        <path className="pet-body" d="M44 75c0-34 25-57 54-57 31 0 55 22 55 57v40c0 27-22 43-55 43-31 0-54-15-54-43Z" />
        <path className="pet-ear" d="M48 61 52 20l31 23Z" />
        <path className="pet-ear" d="m145 61-4-41-30 23Z" />
        <path className="pet-face" d="M60 72c13-14 63-14 76 0v34c0 23-15 36-38 36-24 0-38-13-38-36Z" />
        <ellipse className="pet-eye" cx="79" cy="91" rx="6" ry="8" />
        <ellipse className="pet-eye" cx="117" cy="91" rx="6" ry="8" />
        <path className="pet-mouth" d="M91 108c4 4 10 4 14 0" />
        <path className="pet-page" d="M69 118h58l-7 32H76Z" />
        <path className="pet-page-line" d="M84 129h27M82 137h33" />
        <path className="pet-sprout" d="M98 32c-2-13 8-19 18-20-1 11-7 19-18 20Z" />
        <path className="pet-sprout" d="M97 32c-9-10-17-10-25-5 6 9 14 12 25 5Z" />
        <path className="pet-tool" d="m135 55 13-13 7 7-13 13-7 1Z" />
      </svg>
    </div>
  );
}
