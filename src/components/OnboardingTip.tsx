import { Alert } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { dismissOnboardingTip, getDismissedOnboardingTips } from "@/services/api";

export type OnboardingTipKey =
  | "proxy"
  | "mcp"
  | "prompts"
  | "skills"
  | "sessions"
  | "usage"
  | "usage_codex_local"
  | "usage_currency"
  | "usage_cache_pricing"
  | "localization"
  | "environment"
  | "about";

export function OnboardingTip({
  tipKey,
  message,
  description,
  action,
  type = "info",
}: {
  tipKey: OnboardingTipKey;
  message: string;
  description?: string;
  action?: ReactNode;
  type?: "info" | "warning" | "success" | "error";
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
