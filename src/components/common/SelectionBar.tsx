import type { ReactNode } from "react";
import { Button, Space } from "antd";
import { useTranslation } from "react-i18next";

export interface SelectionBarProps {
  count: number;
  onClear: () => void;
  children?: ReactNode;
}

/** Batch actions that appear only after the user selects rows. */
export function SelectionBar({ count, onClear, children }: SelectionBarProps) {
  const { t } = useTranslation();
  if (count <= 0) return null;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 12,
        padding: "8px 12px",
        borderRadius: 8,
        background: "var(--color-bg-selected-subtle, rgba(22, 119, 255, 0.08))",
      }}
    >
      <span>
        {t("common.selectedCount", { count, defaultValue: "已选 {{count}} 项" })}
      </span>
      <Space size={8}>
        {children}
        <Button size="small" onClick={onClear}>
          {t("common.clearSelection", { defaultValue: "取消选择" })}
        </Button>
      </Space>
    </div>
  );
}
