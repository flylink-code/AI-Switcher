import React, { useEffect, useState } from "react";
import appLogo from "@/assets/app-logo.png";
import { useThemeStore } from "@/stores/themeStore";

/** Hide wordmark below this window width to free space for the 6-item dock. */
const BRAND_COMPACT_MQ = "(max-width: 1180px)";

export const AppBrand: React.FC = () => {
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";
  const [compact, setCompact] = useState(() =>
    typeof window !== "undefined" ? window.matchMedia(BRAND_COMPACT_MQ).matches : false,
  );

  useEffect(() => {
    const mq = window.matchMedia(BRAND_COMPACT_MQ);
    const onChange = () => setCompact(mq.matches);
    onChange();
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: compact ? "6px" : "10px",
        paddingLeft: "4px",
        paddingRight: compact ? "4px" : "8px",
        userSelect: "none",
      }}
      title="AI-Switcher"
    >
      <img
        src={appLogo}
        alt="AI-Switcher"
        width={28}
        height={28}
        draggable={false}
        style={{
          width: 28,
          height: 28,
          borderRadius: 8,
          objectFit: "contain",
          flexShrink: 0,
          display: "block",
        }}
      />
      {!compact ? (
        <span
          style={{
            fontSize: "15px",
            fontWeight: 700,
            letterSpacing: "-0.01em",
            color: isDark ? "#F2F4F7" : "#111827",
            whiteSpace: "nowrap",
          }}
        >
          AI-Switcher
        </span>
      ) : null}
    </div>
  );
};
