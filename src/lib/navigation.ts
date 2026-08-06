import { createContext, useContext } from "react";
import type { PageKey } from "@/lib/pageRegistry";

/** Lets any page trigger shell navigation without prop drilling. */
export const NavigationContext = createContext<(key: PageKey) => void>(() => {});

export function useNavigatePage(): (key: PageKey) => void {
  return useContext(NavigationContext);
}
