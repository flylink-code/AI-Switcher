import { useMemo, useState } from "react";
import { Button, Popconfirm, Select, Typography, message } from "antd";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { gatewayTimezoneOptions } from "@/lib/appQueries";
import { errMsg } from "@/lib/errMsg";
import type { AccountDeviceProfile } from "@/types/backend";

const { Text } = Typography;

/** tsconfig 的 lib 为 ES2021，缺 Intl.supportedValuesOf 的类型声明。 */
export function timezoneOptions(): { value: string; label: string }[] {
  const zones = (Intl as unknown as { supportedValuesOf(key: "timeZone"): string[] }).supportedValuesOf("timeZone");
  return zones.map((zone) => ({ value: zone, label: zone }));
}

/** 账号有效时区：账号覆盖 > 全局 > 浏览器本地（undefined）。 */
export function useEffectiveTimezone(profile: AccountDeviceProfile): string | undefined {
  const globalTimezone = useQuery(gatewayTimezoneOptions).data;
  return profile.timezone || globalTimezone || undefined;
}

export function AccountDeviceRow({
  profile,
  onSave,
}: {
  profile: AccountDeviceProfile;
  onSave: (timezone: string, regenerate: boolean) => Promise<unknown>;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const options = useMemo(
    () => [{ value: "", label: t("device.followGlobal") }, ...timezoneOptions()],
    [t],
  );

  const save = async (timezone: string, regenerate: boolean) => {
    setBusy(true);
    try {
      await onSave(timezone, regenerate);
      message.success(t("device.saved"));
    } catch (error) {
      message.error(errMsg(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: "4px 8px", minHeight: 22 }}>
      <Text type="secondary" style={{ fontSize: 12 }}>
        {t("device.label")}: {profile.os}/{profile.arch} · {profile.deviceId.slice(0, 8)}
      </Text>
      <Select
        size="small"
        showSearch
        optionFilterProp="label"
        value={profile.timezone}
        loading={busy}
        disabled={busy}
        style={{ width: 170 }}
        options={options}
        onChange={(timezone) => void save(timezone, false)}
      />
      <Popconfirm
        title={t("device.regenerateConfirm")}
        okText={t("device.regenerate")}
        cancelText={t("common.cancel")}
        onConfirm={() => void save(profile.timezone, true)}
      >
        <Button size="small" loading={busy}>{t("device.regenerate")}</Button>
      </Popconfirm>
    </div>
  );
}
