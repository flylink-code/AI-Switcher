import React from "react";
import { Button, Tooltip } from "antd";

export interface IconButtonProps {
  icon: React.ReactNode;
  title: string;
  onClick?: (event: React.MouseEvent<HTMLElement>) => void;
  danger?: boolean;
  disabled?: boolean;
  loading?: boolean;
  className?: string;
  style?: React.CSSProperties;
}

export const IconButton: React.FC<IconButtonProps> = ({
  icon,
  title,
  onClick,
  danger = false,
  disabled = false,
  loading = false,
  className = "",
  style,
}) => {
  const btn = (
    <Button
      type="text"
      size="small"
      icon={icon}
      danger={danger}
      disabled={disabled}
      loading={loading}
      onClick={onClick}
      aria-label={title}
      className={className}
      style={{
        borderRadius: "var(--radius-sm)",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        ...style,
      }}
    />
  );

  if (title && !disabled) {
    return <Tooltip title={title}>{btn}</Tooltip>;
  }

  return btn;
};
