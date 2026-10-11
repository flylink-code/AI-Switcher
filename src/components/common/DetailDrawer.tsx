import type { ReactNode } from "react";
import { Drawer } from "antd";

export interface DetailDrawerProps {
  open: boolean;
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  width?: number;
  extra?: ReactNode;
}

/** Right-side detail drawer used for viewing or editing a single item. */
export function DetailDrawer({
  open,
  title,
  onClose,
  children,
  width = 440,
  extra,
}: DetailDrawerProps) {
  return (
    <Drawer
      open={open}
      title={title}
      extra={extra}
      onClose={onClose}
      width={width}
      destroyOnClose
    >
      {children}
    </Drawer>
  );
}
