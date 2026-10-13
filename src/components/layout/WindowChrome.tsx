import { getCurrentWindow } from "@tauri-apps/api/window";
import { theme } from "antd";
import { TitlebarActions } from "./TitlebarActions";

const appWindow = getCurrentWindow();

export const WINDOW_CHROME_HEIGHT = 48;

export interface WindowChromeProps {
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
  extraLeft?: React.ReactNode;
}

export function WindowChrome({ updateVersion, onOpenUpdate, extraLeft }: WindowChromeProps) {
  const { token } = theme.useToken();

  return (
    <div
      className="workspace-window-chrome"
      style={{
        height: WINDOW_CHROME_HEIGHT,
        minHeight: WINDOW_CHROME_HEIGHT,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "space-between",
        paddingLeft: "var(--page-padding-x, 16px)",
        paddingRight: 0,
        backgroundColor: token.colorBgContainer,
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
        userSelect: "none",
        boxSizing: "border-box",
      }}
    >
      <div
        data-tauri-drag-region
        onDoubleClick={(e) => {
          if ((e.target as HTMLElement).closest("button, select, input, .no-drag")) return;
          void appWindow.toggleMaximize();
        }}
        style={{
          flex: 1,
          height: "100%",
          display: "flex",
          alignItems: "center",
          gap: "12px",
          minWidth: 0,
        }}
      >
        {extraLeft}
      </div>

      <TitlebarActions updateVersion={updateVersion} onOpenUpdate={onOpenUpdate} />
    </div>
  );
}
