import { Button, Progress, Space, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { StartupProgress } from "@/lib/startupWarmup";

const { Text, Title } = Typography;

export function StartupScreen({
  progress,
  onSkip,
}: {
  progress: StartupProgress;
  onSkip: () => void;
}) {
  const { t } = useTranslation();
  const percent = Math.round((progress.completed / Math.max(progress.total, 1)) * 100);
  const taskKey = `startup.tasks.${progress.current}`;
  return (
    <div className="startup-screen">
      <Space direction="vertical" size="large" align="center" style={{ width: "100%" }}>
        <div className="startup-mark">CS</div>
        <Space direction="vertical" size={4} align="center">
          <Title level={3} style={{ margin: 0 }}>{t("app.name")}</Title>
          <Text type="secondary">{t("startup.preparing")}</Text>
        </Space>
        <Progress percent={percent} status="active" style={{ maxWidth: 440 }} />
        <Text type="secondary">
          {t(taskKey, { defaultValue: t("startup.preparing") })}
        </Text>
        {progress.failures.length > 0 && (
          <Text type="warning">
            {t("startup.backgroundPending", { count: progress.failures.length })}
          </Text>
        )}
        <Button type="text" onClick={onSkip}>{t("startup.skip")}</Button>
      </Space>
    </div>
  );
}
