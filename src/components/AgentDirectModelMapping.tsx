import { useMemo, useState } from "react";
import { AutoComplete, Button, Space, Typography, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ExpandableSection } from "@/components/common/ExpandableSection";
import { errMsg } from "@/lib/errMsg";
import {
  getAgentDirectModelMapping,
  listGatewayUpstreamModels,
  setAgentDirectModelMapping,
} from "@/services/providers";
import type { ClaudeModelMapping, ProviderTarget } from "@/types/backend";

const { Text } = Typography;

type MappingRole = keyof ClaudeModelMapping;

const ROLES: readonly MappingRole[] = ["sonnet", "opus", "haiku", "fable", "subagent"];

/** Targets whose direct write path consumes role mappings. */
export function supportsDirectModelMapping(target: ProviderTarget): boolean {
  switch (target) {
    case "claude_code":
    case "claude_desktop":
      return true;
    case "codex":
    case "opencode":
    case "pi":
    case "cline":
    case "dsh":
      return false;
    default: {
      const _exhaustive: never = target;
      return _exhaustive;
    }
  }
}

function emptyMapping(): ClaudeModelMapping {
  return { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" };
}

function roleLabel(role: MappingRole, t: (key: string, options?: { defaultValue: string }) => string): string {
  switch (role) {
    case "sonnet":
      return "Sonnet";
    case "opus":
      return "Opus";
    case "haiku":
      return "Haiku";
    case "fable":
      return "Fable";
    case "subagent":
      return t("providers.agentMappingSubagent", { defaultValue: "子代理" });
    default: {
      const _exhaustive: never = role;
      return _exhaustive;
    }
  }
}

interface MappingEditorProps {
  target: ProviderTarget;
  initial: ClaudeModelMapping;
  modelOptions: string[];
  disabled: boolean;
}

function MappingEditor({ target, initial, modelOptions, disabled }: MappingEditorProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<ClaudeModelMapping>(initial);
  const [saving, setSaving] = useState(false);
  const options = useMemo(() => modelOptions.map((value) => ({ value })), [modelOptions]);
  const dirty = ROLES.some((role) => draft[role].trim() !== initial[role].trim());
  const hasAny = ROLES.some((role) => draft[role].trim() !== "");

  const save = async (next: ClaudeModelMapping) => {
    setSaving(true);
    try {
      const saved = await setAgentDirectModelMapping(target, next);
      queryClient.setQueryData(["agent-direct-model-mapping", target], saved);
      setDraft(saved);
      await queryClient.invalidateQueries({ queryKey: ["agent-config-drift"] });
      void message.success(t("providers.agentMappingSaved"));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <Text type="secondary" style={{ fontSize: 12 }}>
        {t("providers.agentMappingHint")}
      </Text>
      {ROLES.map((role) => (
        <div key={role} style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Text style={{ width: 72, flexShrink: 0 }}>{roleLabel(role, t)}</Text>
          <AutoComplete
            size="small"
            style={{ flex: 1, minWidth: 0 }}
            value={draft[role]}
            disabled={disabled || saving}
            options={options}
            placeholder={t("providers.agentMappingPlaceholder")}
            filterOption={(input, option) =>
              String(option?.value ?? "").toLowerCase().includes(input.trim().toLowerCase())
            }
            onChange={(value) => setDraft((current) => ({ ...current, [role]: String(value ?? "") }))}
          />
        </div>
      ))}
      <Space size={8}>
        <Button
          size="small"
          type="primary"
          loading={saving}
          disabled={disabled || !dirty}
          onClick={() => void save(draft)}
        >
          {t("common.save")}
        </Button>
        <Button
          size="small"
          disabled={disabled || saving || !hasAny}
          onClick={() => void save(emptyMapping())}
        >
          {t("providers.clearModelMapping")}
        </Button>
      </Space>
    </div>
  );
}

export interface AgentDirectModelMappingProps {
  target: ProviderTarget;
  upstreamId: string;
  disabled?: boolean;
}

/** Per-agent role mapping for a direct connection (Claude Code / Desktop). */
export function AgentDirectModelMapping({ target, upstreamId, disabled = false }: AgentDirectModelMappingProps) {
  const { t } = useTranslation();
  const mappingQuery = useQuery({
    queryKey: ["agent-direct-model-mapping", target],
    queryFn: () => getAgentDirectModelMapping(target),
  });
  const modelsQuery = useQuery({
    queryKey: ["gateway-upstream-models", upstreamId],
    queryFn: () => listGatewayUpstreamModels(upstreamId),
    enabled: Boolean(upstreamId),
  });
  const modelOptions = useMemo(
    () => (modelsQuery.data ?? []).map((row) => row.modelId),
    [modelsQuery.data],
  );

  return (
    <ExpandableSection title={t("providers.agentMappingTitle")}>
      {mappingQuery.data ? (
        <MappingEditor
          key={`${target}:${upstreamId}:${JSON.stringify(mappingQuery.data)}`}
          target={target}
          initial={mappingQuery.data}
          modelOptions={modelOptions}
          disabled={disabled}
        />
      ) : null}
    </ExpandableSection>
  );
}
