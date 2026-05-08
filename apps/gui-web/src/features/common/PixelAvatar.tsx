import clsx from "clsx";

const palette = [
  "var(--brutal-cyan)",
  "var(--brutal-lime)",
  "var(--brutal-lavender)",
  "var(--brutal-orange)",
  "var(--brutal-pink)",
];

export function PixelAvatar({
  id,
  label,
  size = 36,
  className,
}: {
  id: string;
  label?: string;
  size?: number;
  className?: string;
}) {
  const cells = buildCells(id || label || "actor");
  const color = palette[hash(id || label || "actor") % palette.length];

  return (
    <span
      className={clsx(
        "inline-grid shrink-0 overflow-hidden border-2 border-black bg-white",
        className,
      )}
      style={{
        width: size,
        height: size,
        gridTemplateColumns: "repeat(8, 1fr)",
        gridTemplateRows: "repeat(8, 1fr)",
        imageRendering: "pixelated",
      }}
      title={label ?? id}
      aria-label={label ?? id}
    >
      {cells.map((filled, i) => (
        <span
          key={i}
          style={{
            backgroundColor: filled
              ? i % 11 === 0
                ? "#ffffff"
                : i % 7 === 0
                  ? "#111111"
                  : color
              : "transparent",
          }}
        />
      ))}
    </span>
  );
}

function buildCells(seed: string): boolean[] {
  const h = hash(seed);
  const cells = Array.from({ length: 64 }, () => false);
  for (let y = 0; y < 8; y += 1) {
    for (let x = 0; x < 4; x += 1) {
      const edge = y === 0 || y === 7 || x === 0;
      const bit = ((h >> ((x + y * 4) % 24)) & 1) === 1;
      const filled = edge || bit;
      cells[y * 8 + x] = filled;
      cells[y * 8 + (7 - x)] = filled;
    }
  }
  cells[3 * 8 + 2] = false;
  cells[3 * 8 + 5] = false;
  cells[5 * 8 + 3] = true;
  cells[5 * 8 + 4] = true;
  return cells;
}

function hash(input: string): number {
  let h = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
