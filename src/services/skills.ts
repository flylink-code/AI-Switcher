import { call } from "./ipc";
import type {
  Agent,
  AgentDraft,
  CodexMarketplaceListResult,
  CodexPluginCommandResult,
  CodexPluginsSnapshot,
  CodexWebSearchMode,
  CodexWebSearchSnapshot,
  RepositorySkill,
  Skill,
  SkillRepositorySnapshot,
  SkillTarget,
  SkillUpdateStatus,
  UnmanagedSkill,
} from "@/types/backend";

export async function listSkills(target?: SkillTarget): Promise<Skill[]> {
  return call<Skill[]>("list_skills", { target });
}

export async function scanUnmanagedSkills(target?: SkillTarget): Promise<UnmanagedSkill[]> {
  return call<UnmanagedSkill[]>("scan_unmanaged_skills", { target });
}

export async function registerUnmanagedSkill(path: string, target?: SkillTarget): Promise<Skill> {
  return call<Skill>("register_unmanaged_skill", { path, target });
}

export async function ignoreUnmanagedSkill(path: string): Promise<void> {
  return call<void>("ignore_unmanaged_skill", { path });
}

// ---- Agents -----------------------------------------------------------------

export async function listAgents(): Promise<Agent[]> {
  return call<Agent[]>("list_agents", {});
}

export async function saveAgent(draft: AgentDraft): Promise<Agent> {
  return call<Agent>("save_agent", { draft });
}

export async function setAgentEnabled(name: string, enabled: boolean): Promise<void> {
  return call<void>("set_agent_enabled", { name, enabled });
}

export async function deleteAgent(name: string): Promise<void> {
  return call<void>("delete_agent", { name });
}

export async function installZipAgent(path: string): Promise<Agent[]> {
  return call<Agent[]>("install_zip_agent", { path });
}

// ---- Codex Plugins ----------------------------------------------------------

export async function listCodexPlugins(): Promise<CodexPluginsSnapshot> {
  return call<CodexPluginsSnapshot>("list_codex_plugins", {});
}

export async function setCodexPluginEnabled(pluginId: string, enabled: boolean): Promise<void> {
  return call<void>("set_codex_plugin_enabled", { pluginId, enabled });
}

export async function listCodexPluginMarketplaces(): Promise<CodexMarketplaceListResult> {
  return call<CodexMarketplaceListResult>("list_codex_plugin_marketplaces", {});
}

export async function addCodexPluginMarketplace(source: string): Promise<CodexPluginCommandResult> {
  return call<CodexPluginCommandResult>("add_codex_plugin_marketplace", { source });
}

export async function removeCodexPluginMarketplace(name: string): Promise<CodexPluginCommandResult> {
  return call<CodexPluginCommandResult>("remove_codex_plugin_marketplace", { name });
}

export async function uninstallCodexPlugin(pluginId: string): Promise<CodexPluginCommandResult> {
  return call<CodexPluginCommandResult>("uninstall_codex_plugin", { pluginId });
}

export async function getCodexWebSearchMode(): Promise<CodexWebSearchSnapshot> {
  return call<CodexWebSearchSnapshot>("get_codex_web_search_mode", {});
}

export async function setCodexWebSearchMode(mode: CodexWebSearchMode): Promise<CodexWebSearchSnapshot> {
  return call<CodexWebSearchSnapshot>("set_codex_web_search_mode", { mode });
}

export async function getSkillRepository(): Promise<string> {
  return call<string>("get_skill_repository", {});
}

export async function getSkillRepositorySnapshot(): Promise<SkillRepositorySnapshot> {
  return call<SkillRepositorySnapshot>("get_skill_repository_snapshot", {});
}

export async function setSkillRepository(url: string): Promise<string> {
  return call<string>("set_skill_repository", { url });
}

export async function listGithubRepositorySkills(url: string): Promise<RepositorySkill[]> {
  return call<RepositorySkill[]>("list_github_repository_skills", { url });
}

export async function refreshGithubRepositorySkills(url: string): Promise<SkillRepositorySnapshot> {
  return call<SkillRepositorySnapshot>("refresh_github_repository_skills", { url });
}

export async function installGithubRepositorySkills(url: string, paths: string[], target?: SkillTarget): Promise<Skill[]> {
  return call<Skill[]>("install_github_repository_skills", { url, paths, target });
}

export async function installGithubSkill(url: string, target?: SkillTarget): Promise<Skill> {
  return call<Skill>("install_github_skill", { url, target });
}

export async function checkSkillUpdate(name: string, target?: SkillTarget): Promise<SkillUpdateStatus> {
  return call<SkillUpdateStatus>("check_skill_update", { name, target });
}

export async function checkSkillUpdates(target?: SkillTarget): Promise<SkillUpdateStatus[]> {
  return call<SkillUpdateStatus[]>("check_skill_updates", { target });
}

export async function updateGithubSkills(names: string[], target?: SkillTarget): Promise<Skill[]> {
  return call<Skill[]>("update_github_skills", { names, target });
}

export async function installZipSkill(path: string, target?: SkillTarget): Promise<Skill> {
  return call<Skill>("install_zip_skill", { path, target });
}

export async function setSkillEnabled(name: string, enabled: boolean, target?: SkillTarget): Promise<void> {
  return call<void>("set_skill_enabled", { name, enabled, target });
}

export async function deleteSkill(name: string, target?: SkillTarget): Promise<void> {
  return call<void>("delete_skill", { name, target });
}

// ---- System -----------------------------------------------------------------
