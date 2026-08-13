import { useEffect, useState } from "react";
import { Space } from "antd";
import { useTranslation } from "react-i18next";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { SkillTarget } from "@/types/backend";
import ClaudePluginsPage from "@/pages/ClaudePluginsPage";
import CodexPluginsPage from "@/pages/CodexPluginsPage";

const PLUGINS_TARGET_KEY = "cs.pluginsTarget";

function readPluginsTarget(): SkillTarget {
  if (typeof localStorage === "undefined") return "claude_code";
  const stored = localStorage.getItem(PLUGINS_TARGET_KEY);
  if (stored === "codex" || stored === "claude_code") return stored;
  return "claude_code";
}

/**
 * Unified plugins hub: Claude Code / Codex share one workspace tab,
 * switched the same way Skills uses WorkspaceTargetSegmented.
 */
export default function PluginsPage() {
  const { t } = useTranslation();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const getValidPluginTarget = (preferred: SkillTarget): SkillTarget => {
    const validTargets = visibleAgents.filter((a): a is SkillTarget => a === "claude_code" || a === "codex");
    if (validTargets.includes(preferred)) return preferred;
    return validTargets[0] ?? "claude_code";
  };

  const [target, setTarget] = useState<SkillTarget>(() => getValidPluginTarget(readPluginsTarget()));

  useEffect(() => {
    const valid = getValidPluginTarget(target);
    if (valid !== target) {
      setTarget(valid);
    }
  }, [visibleAgents, target]);

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(PLUGINS_TARGET_KEY, target);
    }
  }, [target]);

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <WorkspaceTargetSegmented<SkillTarget>
        value={target}
        onChange={setTarget}
        t={t}
        targets={["claude_code", "codex"]}
      />
      {target === "claude_code" ? <ClaudePluginsPage /> : <CodexPluginsPage />}
    </Space>
  );
}
