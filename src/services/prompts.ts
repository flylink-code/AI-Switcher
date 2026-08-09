import { call } from "./ipc";
import type {
  LivePrompt,
  PromptDetail,
  PromptInfo,
  PromptTarget,
} from "@/types/backend";

export async function listPrompts(target: PromptTarget = "claude_code"): Promise<PromptInfo[]> {
  return call<PromptInfo[]>("list_prompts", { target });
}

export async function readPrompt(name: string, target: PromptTarget = "claude_code"): Promise<PromptDetail> {
  return call<PromptDetail>("read_prompt", { name, target });
}

export async function savePrompt(name: string, content: string, target: PromptTarget = "claude_code"): Promise<void> {
  return call<void>("save_prompt", { name, content, target });
}

export async function renamePrompt(
  oldName: string,
  newName: string,
  target: PromptTarget = "claude_code",
): Promise<void> {
  return call<void>("rename_prompt", { oldName, newName, target });
}

export async function deletePrompt(name: string, target: PromptTarget = "claude_code"): Promise<void> {
  return call<void>("delete_prompt", { name, target });
}

export async function activatePrompt(name: string, target: PromptTarget = "claude_code"): Promise<void> {
  return call<void>("activate_prompt", { name, target });
}

export async function readLivePrompt(target: PromptTarget = "claude_code"): Promise<LivePrompt | null> {
  return call<LivePrompt | null>("read_live_prompt", { target });
}

export async function importLivePrompt(name: string, target: PromptTarget = "claude_code"): Promise<void> {
  return call<void>("import_live_prompt", { name, target });
}

// ---- Skills -----------------------------------------------------------------
