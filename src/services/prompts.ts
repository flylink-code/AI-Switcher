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

export async function listPiPromptTemplates(): Promise<string[]> {
  return call<string[]>("list_pi_prompt_templates", {});
}

export async function readPiPromptTemplate(name: string): Promise<string> {
  return call<string>("read_pi_prompt_template", { name });
}

export async function savePiPromptTemplate(name: string, content: string): Promise<void> {
  return call<void>("save_pi_prompt_template", { name, content });
}

export async function deletePiPromptTemplate(name: string): Promise<void> {
  return call<void>("delete_pi_prompt_template", { name });
}

export async function getWorkspacePiPrompt(workspaceDir: string): Promise<[string, string] | null> {
  return call<[string, string] | null>("get_workspace_pi_prompt", { workspaceDir });
}

export async function saveWorkspacePiPrompt(workspaceDir: string, fileName: string, content: string): Promise<void> {
  return call<void>("save_workspace_pi_prompt", { workspaceDir, fileName, content });
}

// ---- Skills -----------------------------------------------------------------
