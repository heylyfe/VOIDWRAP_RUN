// Per-class accent colours for ability visuals (striker keeps DashStreak).
import { CLASS_BULWARK, CLASS_GLIDER, CLASS_GRAPPLER, CLASS_STRIKER } from "@/protocol/generated";

/**
 * Per-class identity colours. Exported so `classColors.test.ts` can assert every one
 * of them actually reaches a renderer -- T-147 found `CLASS_STRIKER` defined here and
 * never read, because the dash streak used the theme accent instead.
 */
export const CLASS_COLORS: Partial<Record<number, string>> = {
  [CLASS_STRIKER]: "#ffb347",
  [CLASS_GRAPPLER]: "#ffe36e",
  [CLASS_BULWARK]: "#c39bff",
  [CLASS_GLIDER]: "#7de6ff",
};

export function classColor(cls: number): string {
  return CLASS_COLORS[cls] ?? "#ffffff";
}
