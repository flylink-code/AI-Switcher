import { Alert } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { CSSProperties, ReactNode } from "react";
import { dismissOnboardingTip, getDismissedOnboardingTips } from "@/services/api";

export type OnboardingTipKey =
  | "proxy"
  | "mcp"
  | "prompts"
  | "skills"
  | "agents"
  | "sessions"
  | "usage"
  | "usage_codex_local"
  | "usage_claude_code_local"
  | "usage_currency"
  | "usage_cache_pricing"
  | "localization"
  | "localization_safe_mode"
  | "localization_third_party"
  | "environment"
  | "environment_sync"
  | "providers_codex_auth"
  | "providers_hot_switch"
  | "about";

export function OnboardingTip({
  tipKey,
  message,
  description,
  action,
  type = "info",
  style,
}: {
  tipKey: OnboardingTipKey;
  message: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
  type?: "info" | "warning" | "success" | "error";
  style?: CSSProperties;
}) {
  const queryClient = useQueryClient();
  const tipsQuery = useQuery({
    queryKey: ["dismissed-onboarding-tips"],
    queryFn: getDismissedOnboardingTips,
    staleTime: Infinity,
  });
  const dismissed = tipsQuery.data?.includes(tipKey) ?? false;

  if (!tipsQuery.data || dismissed) return null;

  return (
    <Alert
      type={type}
      showIcon
      closable
      message={message}
      description={description}
      action={action}
      style={style}
      onClose={() => {
        void dismissOnboardingTip(tipKey).then(() => {
          queryClient.setQueryData<string[]>(["dismissed-onboarding-tips"], (current = []) =>
            current.includes(tipKey) ? current : [...current, tipKey],
          );
        }).catch(() => {
          void queryClient.invalidateQueries({ queryKey: ["dismissed-onboarding-tips"] });
        });
      }}
    />
  );
}
