import { useMemo } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Popconfirm,
  Skeleton,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  getDesktopLocalizationStatus,
  installDesktopLocalization,
  restoreDesktopLocalization,
  selectDesktopLocalizationPack,
  validateDesktopLocalizationPack,
} from "@/services/api";

const { Text } = Typography;
const localizationKey = ["environment", "desktop-localization"] as const;

function PathValue({ value }: { value?: string | null }) {
  const { t } = useTranslation();
  if (!value) return <Tag>{t("env.notDetected")}</Tag>;
  return <Text copyable code style={{ wordBreak: "break-all" }}>{value}</Text>;
}

export default function DesktopLocalizationPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: localizationKey,
    queryFn: getDesktopLocalizationStatus,
    staleTime: 60_000,
  });
  const localization = statusQuery.data;

  const refreshStatus = async () => {
    await queryClient.invalidateQueries({ queryKey: localizationKey });
  };

  const selectPack = useMutation({
    mutationFn: async () => {
      const path = await selectDesktopLocalizationPack();
      return path ? validateDesktopLocalizationPack(path) : null;
    },
    onSuccess: async (result) => {
      if (!result) return;
      void message.success(result.message);
      await refreshStatus();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const install = useMutation({
    mutationFn: async () => {
      if (!localization?.packPath) throw new Error(t("env.localization.selectPack"));
      return installDesktopLocalization(localization.packPath);
    },
    onSuccess: async (result) => {
      void message.success(result.message);
      await refreshStatus();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshStatus();
    },
  });

  const restore = useMutation({
    mutationFn: restoreDesktopLocalization,
    onSuccess: async (result) => {
      void message.success(result.message);
      await refreshStatus();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshStatus();
    },
  });

  const busy = selectPack.isPending || install.isPending || restore.isPending;
  const diagnostics = useMemo(
    () => localization?.diagnostics.filter(Boolean).join("\n") ?? "",
    [localization?.diagnostics],
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Alert
        type="info"
        showIcon
        message={t("env.localization.safeMode")}
        description={t("env.localization.safeModeDescription")}
      />
      <Card
        size="small"
        title={t("env.localization.title")}
        extra={
          <Button
            size="small"
            icon={<ReloadOutlined spin={statusQuery.isFetching} />}
            disabled={busy}
            onClick={() => void statusQuery.refetch()}
          >
            {t("common.refresh")}
          </Button>
        }
      >
        {statusQuery.isPending ? (
          <Skeleton active paragraph={{ rows: 6 }} />
        ) : statusQuery.error ? (
          <Alert type="error" showIcon message={errorMessage(statusQuery.error)} />
        ) : (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            {localization?.multipleInstalls && (
              <Alert type="warning" showIcon message={t("env.localization.multipleInstalls")} />
            )}
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("env.localization.status")}>
                <Tag
                  color={
                    localization?.state === "installed"
                      ? "green"
                      : localization?.state === "partial"
                        ? "orange"
                        : "default"
                  }
                >
                  {localization
                    ? t(`env.localization.states.${localization.state}`)
                    : t("env.localization.loading")}
                </Tag>
                {localization?.message && <Text type="secondary"> {localization.message}</Text>}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.version")}>
                {localization?.claudeVersion ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.installPath")}>
                <PathValue value={localization?.installPath} />
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.locale")}>
                {localization?.configuredLocale ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.packPath")}>
                <PathValue value={localization?.packPath} />
              </Descriptions.Item>
            </Descriptions>
            {!localization?.installDetected && diagnostics && (
              <Alert
                type="warning"
                showIcon
                message={localization?.message ?? t("env.localization.statusFailed")}
                description={<pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{diagnostics}</pre>}
              />
            )}
            <Space wrap>
              <Button
                icon={<FolderOpenOutlined />}
                loading={selectPack.isPending}
                disabled={busy && !selectPack.isPending}
                onClick={() => selectPack.mutate()}
              >
                {t("env.localization.selectPack")}
              </Button>
              <Popconfirm
                title={t("env.localization.confirmInstall")}
                description={t("env.localization.confirmInstallDescription")}
                onConfirm={() => install.mutate()}
              >
                <Button
                  type="primary"
                  loading={install.isPending}
                  disabled={
                    busy ||
                    !localization?.platformSupported ||
                    !localization.installDetected ||
                    !localization.packValid
                  }
                >
                  {t("env.localization.install")}
                </Button>
              </Popconfirm>
              <Popconfirm
                title={t("env.localization.confirmRestore")}
                onConfirm={() => restore.mutate()}
              >
                <Button
                  danger
                  loading={restore.isPending}
                  disabled={busy || !localization?.backupAvailable}
                >
                  {t("env.localization.restore")}
                </Button>
              </Popconfirm>
            </Space>
          </Space>
        )}
      </Card>
    </Space>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
