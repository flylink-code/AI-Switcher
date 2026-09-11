import { useState } from "react";
import { App, Badge, Tooltip } from "antd";
import LoadingOutlined from "@ant-design/icons/es/icons/LoadingOutlined";
import { useTranslation } from "react-i18next";
import { useAppUpdatePrompt } from "@/lib/appUpdateContext";
import { runAppUpdateCheck, type AppUpdateCheckResult } from "@/lib/appUpdater";
import { useAppVersion } from "@/lib/useAppVersion";

export function TitlebarVersionButton({
  updateVersion,
  onOpenUpdate,
}: {
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
}) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { presentUpdate } = useAppUpdatePrompt();
  const appVersion = useAppVersion();
  const [checking, setChecking] = useState(false);

  const notifyResult = (outcome: AppUpdateCheckResult) => {
    switch (outcome.kind) {
      case "available":
        presentUpdate(outcome.update);
        return;
      case "upToDate":
        void message.info(t("about.appUpToDate"));
        return;
      case "packagePending":
        void message.warning(t("about.appUpdatePackagePending"));
        return;
      case "failed":
        void message.error(t("about.appUpdateFailedDetail", { error: outcome.error }));
        return;
      default: {
        const _exhaustive: never = outcome;
        return _exhaustive;
      }
    }
  };

  const handleClick = async () => {
    if (updateVersion) {
      onOpenUpdate?.();
      return;
    }
    if (checking) return;
    setChecking(true);
    try {
      notifyResult(await runAppUpdateCheck(t("about.appUpdateTimedOut")));
    } finally {
      setChecking(false);
    }
  };

  if (updateVersion) {
    return (
      <Badge dot offset={[-2, 4]}>
        <button
          type="button"
          className="app-titlebar-update"
          onClick={() => void handleClick()}
        >
          {t("about.appUpdateAvailable", { version: updateVersion })}
        </button>
      </Badge>
    );
  }

  const label = appVersion ? `v${appVersion}` : "v—";
  const tooltip = appVersion
    ? t("about.titlebarVersionTooltip", {
        version: appVersion,
        defaultValue: `当前 v${appVersion}，点击检查更新`,
      })
    : t("about.titlebarVersionTooltipUnknown", { defaultValue: "点击检查更新" });

  return (
    <Tooltip title={checking ? t("about.checkAppUpdate") : tooltip}>
      <button
        type="button"
        className="app-titlebar-version"
        disabled={checking}
        onClick={() => void handleClick()}
        aria-label={tooltip}
      >
        {checking ? <LoadingOutlined spin style={{ fontSize: 11 }} /> : null}
        <span>{label}</span>
      </button>
    </Tooltip>
  );
}
