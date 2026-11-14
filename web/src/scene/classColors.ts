// Per-class accent colours for ability visuals (striker keeps DashStreak).
import { CLASS_BULWARK, CLASS_GLIDER, CLASS_GRAPPLER, CLASS_STRIKER } from "@/protocol/generated";

const CLASS_COLORS: Partial<Record<number, string>> = {
  [CLASS_STRIKER]: "#ffb347",
  [CLASS_GRAPPLER]: "#ffe36e",
  [CLASS_BULWARK]: "#c39bff",
  [CLASS_GLIDER]: "#7de6ff",
};

export function classColor(cls: number): string {
  return CLASS_COLORS[cls] ?? "#ffffff";
}
