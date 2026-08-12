import React from "react";
import type { PageKey } from "@/lib/pageRegistry";
import WorkbenchPage from "@/pages/WorkbenchPage";

export interface DashboardV2Props {
  /** Kept for ActivePage symmetry; WorkbenchPage uses NavigationContext. */
  onNavigate?: (key: PageKey) => void;
}

/**
 * V2 dashboard shell — reuses the V1 Usage Intelligence overview
 * (status strip → 24h hero → attention/activity → year heatmap).
 * Proxy deep-links still navigate("proxy") and surface under Settings in V2.
 */
export const DashboardV2: React.FC<DashboardV2Props> = () => {
  return <WorkbenchPage />;
};
