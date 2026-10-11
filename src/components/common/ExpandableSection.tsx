import { useState, type ReactNode } from "react";
import RightOutlined from "@ant-design/icons/es/icons/RightOutlined";

export interface ExpandableSectionProps {
  title: ReactNode;
  children: ReactNode;
  /** Initial open state. Not persisted. */
  defaultOpen?: boolean;
  extra?: ReactNode;
}

/** Inline disclosure for infrequent settings. Open state is local to the mount. */
export function ExpandableSection({
  title,
  children,
  defaultOpen = false,
  extra,
}: ExpandableSectionProps) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <button
          type="button"
          onClick={() => setOpen((value) => !value)}
          aria-expanded={open}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: 0,
            border: "none",
            background: "transparent",
            color: "inherit",
            cursor: "pointer",
            font: "inherit",
          }}
        >
          <RightOutlined
            style={{
              fontSize: 10,
              transform: open ? "rotate(90deg)" : undefined,
              transition: "transform 0.15s ease",
            }}
          />
          {title}
        </button>
        {extra}
      </div>
      {open ? <div style={{ marginTop: 12 }}>{children}</div> : null}
    </div>
  );
}
