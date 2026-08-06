import type { MessageInstance } from "antd/es/message/interface";
import type { CodexSwitchNotice } from "@/types/backend";

/** Surface the Codex auth-mode hint returned by provider switching. */
export function showCodexSwitchNotice(
  notice: CodexSwitchNotice | null | undefined,
  message: MessageInstance,
  t: (key: string) => string,
) {
  if (notice === "preserved_official_login") {
    void message.info(t("providers.codexPreservedOfficialLogin"), 5);
  } else if (notice === "official_login_required") {
    void message.warning(t("providers.codexOfficialLoginRequired"), 8);
  }
}
