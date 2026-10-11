import { Button, Dropdown, type MenuProps } from "antd";
import MoreOutlined from "@ant-design/icons/es/icons/MoreOutlined";

export interface MoreMenuProps {
  items: MenuProps["items"];
  ariaLabel?: string;
  disabled?: boolean;
}

/** Overflow menu for secondary row and toolbar actions. */
export function MoreMenu({ items, ariaLabel = "more", disabled }: MoreMenuProps) {
  return (
    <Dropdown menu={{ items }} trigger={["click"]} disabled={disabled}>
      <Button
        type="text"
        size="small"
        icon={<MoreOutlined />}
        aria-label={ariaLabel}
        disabled={disabled}
      />
    </Dropdown>
  );
}
