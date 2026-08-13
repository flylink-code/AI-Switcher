import { createContext, useContext } from "react";
import type { AppUpdate } from "@/lib/appUpdater";

/** Lets pages present the shared in-app update dialog without Modal.confirm. */
export type AppUpdatePromptApi = {
  presentUpdate: (update: AppUpdate) => void;
};

export const AppUpdatePromptContext = createContext<AppUpdatePromptApi>({
  presentUpdate: () => {},
});

export function useAppUpdatePrompt(): AppUpdatePromptApi {
  return useContext(AppUpdatePromptContext);
}
