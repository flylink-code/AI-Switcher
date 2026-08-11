import React from "react";
import { Alert, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { ProviderTarget, ProxyStatus } from "@/types/backend";
import { Surface, Stack } from "@/components/ui";

const { Text } = Typography;

export interface ProxyRuntimeCardProps {
  status: ProxyStatus | null;
  target: ProviderTarget;
  /** Localized label of the page-local Agent target. */
  clientLabel: string;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * Compact route context under the page header.
 * Agent switcher + start/stop live in the Providers-style page header, not here.
 */
export const ProxyRuntimeCard: React.FC<ProxyRuntimeCardProps> = ({
  status,
  target,
  clientLabel,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const isOpencode = target === "opencode";

  if (isOpencode) {
    return (
      <Surface padding="md" className={className} style={style}>
        <Alert type="info" showIcon message={t("proxy.opencodeDirectHint")} />
      </Surface>
    );
  }

  return (
    <Stack gap="sm" className={className} style={style}>
      <Text className="proxy-context-strip">
        {clientLabel}
        {" → "}
        <Text code style={{ fontSize: "var(--font-size-xs)" }}>
          127.0.0.1:{status?.port ?? "—"}
        </Text>
        {" → "}
        {status?.targetProvider ? (
          <Text strong style={{ fontSize: "var(--font-size-sm)" }}>
            {status.targetProvider}
          </Text>
        ) : (
          t("proxy.noTarget", { defaultValue: "未指定" })
        )}
      </Text>

      {status?.lastError && (
        <Alert type="error" showIcon message={status.lastError} />
      )}
    </Stack>
  );
};
