// Interpolation buffer for remote entities (other players, enemies): render them slightly in
// the past, between the two snapshots that bracket the render time.
import type { Vec3 } from "@/protocol/generated";

/** How far behind real time remote entities are drawn (2 server ticks). */
export const INTERP_DELAY_MS = 100;
const MAX_SNAPSHOTS = 32;

interface Sample {
  t: number;
  positions: Map<number, Vec3>;
}

export class SnapshotBuffer {
  private samples: Sample[] = [];

  push(t: number, entities: Iterable<{ id: number; pos: Vec3 }>): void {
    const positions = new Map<number, Vec3>();
    for (const e of entities) positions.set(e.id, e.pos);
    this.samples.push({ t, positions });
    if (this.samples.length > MAX_SNAPSHOTS) this.samples.shift();
  }

  /** Interpolated positions at `now - INTERP_DELAY_MS`. Entities missing from either side are held. */
  sample(now: number): Map<number, Vec3> {
    const out = new Map<number, Vec3>();
    const s = this.samples;
    if (s.length === 0) return out;
    const t = now - INTERP_DELAY_MS;
    // Newest sample b with the sample a just before it, such that a.t <= t (when possible).
    let i = s.length - 1;
    while (i > 0 && (s[i - 1]?.t ?? -Infinity) > t) i--;
    const b = s[i];
    if (!b) return out;
    const a = s[i - 1] ?? b;
    const span = b.t - a.t;
    const k = span > 0 ? Math.min(Math.max((t - a.t) / span, 0), 1) : 1;
    for (const [id, pb] of b.positions) {
      const pa = a.positions.get(id) ?? pb;
      out.set(id, { x: pa.x + (pb.x - pa.x) * k, y: pa.y + (pb.y - pa.y) * k, z: pa.z + (pb.z - pa.z) * k });
    }
    return out;
  }
}
