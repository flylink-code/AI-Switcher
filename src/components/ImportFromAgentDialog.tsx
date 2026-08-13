import { useEffect, useMemo, useState } from "react";
import { Button, List, Modal, Select, Typography } from "antd";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { Provider, ProviderKind, ProviderTarget } from "@/types/backend";
import { LABEL_KEYS, TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { providerListOptions } from "@/lib/appQueries";

export function canCopyProviderTo(provider: Provider, dest: ProviderTarget): boolean {
  if (provider.targetApp === dest) return false;
  return canCopyProviderKindTo(provider.providerKind, dest);
}

function canCopyProviderKindTo(kind: ProviderKind, dest: ProviderTarget): boolean {
  switch (kind) {
    case "codex_oauth":
      return dest === "claude_code" || dest === "claude_desktop";
    case "standard":
    case "antigravity":
      return true;
    default: {
      const _exhaustive: never = kind;
      return _exhaustive;
    }
  }
}

interface ImportFromAgentDialogProps {
  open: boolean;
  dest: ProviderTarget;
  confirming: boolean;
  onCancel: () => void;
  onImport: (provider: Provider) => void;
}

export function ImportFromAgentDialog({
  open,
  dest,
  confirming,
  onCancel,
  onImport,
}: ImportFromAgentDialogProps) {
  const { t } = useTranslation();
  const sourceOptions = useMemo(
    () => TARGET_OPTIONS.filter((option) => option !== dest),
    [dest],
  );
  const [source, setSource] = useState<ProviderTarget>(sourceOptions[0] ?? "claude_code");

  useEffect(() => {
    if (!open) return;
    setSource((current) => (current === dest ? (sourceOptions[0] ?? "claude_code") : current));
  }, [open, dest, sourceOptions]);

  const query = useQuery({
    ...providerListOptions(source),
    enabled: open,
  });
  const providers = (query.data ?? []).filter((provider) => canCopyProviderTo(provider, dest));

  return (
    <Modal
      open={open}
      title={t("providers.importFromAgentTitle")}
      footer={null}
      onCancel={onCancel}
      destroyOnHidden
      width={560}
    >
      <Typography.Paragraph type="secondary" style={{ marginTop: 0 }}>
        {t("providers.importFromAgentHint")}
      </Typography.Paragraph>
      <Select<ProviderTarget>
        style={{ width: "100%", marginBottom: 12 }}
        value={source}
        onChange={setSource}
        options={sourceOptions.map((option) => ({
          value: option,
          label: t(LABEL_KEYS[option]),
        }))}
        aria-label={t("providers.importFromAgentSource")}
      />
      <List
        loading={query.isFetching}
        locale={{ emptyText: t("providers.importFromAgentEmpty") }}
        dataSource={providers}
        renderItem={(provider) => (
          <List.Item
            actions={[
              <Button
                key="import"
                type="primary"
                size="small"
                loading={confirming}
                onClick={() => onImport(provider)}
              >
                {t("providers.importFromAgentAction")}
              </Button>,
            ]}
          >
            <List.Item.Meta
              title={provider.name}
              description={`${provider.protocolType} · ${provider.model || "—"} · ${provider.baseUrl}`}
            />
          </List.Item>
        )}
      />
    </Modal>
  );
}
