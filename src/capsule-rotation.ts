export interface RotationFrame {
  index: number;
  nextIn: number | null;
}

export function rotationFrame(count: number, elapsedMs: number, intervalSeconds: number): RotationFrame {
  if (count < 2) return { index: 0, nextIn: null };
  const interval = Math.max(intervalSeconds, 30) * 1_000;
  const elapsed = Math.max(0, elapsedMs);
  return {
    index: Math.floor(elapsed / interval) % count,
    nextIn: interval - (elapsed % interval),
  };
}
