import { useCallback, useEffect, useState } from "react";
import { Button, Checkbox, Select, Space, Typography, message } from "antd";
import { useTranslation } from "react-i18next";
import { errMsg } from "@/lib/errMsg";
import {
  getCodexOutputProfile,
  getCodexWebSearchMode,
  setCodexOutputProfile,
  setCodexWebSearchMode,
} from "@/services/api";
import type {
  CodexOutputProfile,
  CodexOutputProfileSnapshot,
  CodexWebSearchMode,
  CodexWebSearchSnapshot,
} from "@/types/backend";

const { Text } = Typography;

/** Codex web search mode and output profile; both live in `~/.codex/config.toml`. */
export function CodexAdvancedSettings() {
  const { t } = useTranslation();
  const [webSearch, setWebSearch] = useState<CodexWebSearchSnapshot | null>(null);
  const [webSearchSaving, setWebSearchSaving] = useState(false);
  const [outputProfile, setOutputProfile] = useState<CodexOutputProfileSnapshot | null>(null);
  const [outputProfileSaving, setOutputProfileSaving] = useState(false);

  const loadWebSearch = useCallback(async () => {
    try {
      setWebSearch(await getCodexWebSearchMode());
    } catch (error) {
      void message.error(errMsg(error));
    }
  }, []);

  const loadOutputProfile = useCallback(async () => {
    try {
      setOutputProfile(await getCodexOutputProfile());
    } catch (error) {
      void message.error(errMsg(error));
    }
  }, []);

  useEffect(() => {
    void loadWebSearch();
    void loadOutputProfile();
  }, [loadWebSearch, loadOutputProfile]);

  const onChangeWebSearch = useCallback(
    async (mode: CodexWebSearchMode) => {
      setWebSearchSaving(true);
      try {
        setWebSearch(await setCodexWebSearchMode(mode));
        void message.success(t("env.webSearchSaved"));
      } catch (error) {
        void message.error(errMsg(error));
      } finally {
        setWebSearchSaving(false);
      }
    },
    [t],
  );

  const onChangeOutputProfile = useCallback(
    async (profile: CodexOutputProfile, showRawReasoning: boolean) => {
      setOutputProfileSaving(true);
      try {
        setOutputProfile(
          await setCodexOutputProfile(profile, profile === "concise" ? false : showRawReasoning),
        );
        void message.success(t("env.outputProfileSaved"));
      } catch (error) {
        void message.error(errMsg(error));
      } finally {
        setOutputProfileSaving(false);
      }
    },
    [t],
  );

  return (
    <Space direction="vertical" size={12} style={{ width: "100%" }}>
      <div>
        <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center" }}>
          <Text strong>{t("env.webSearchTitle")}</Text>
          <Button size="small" onClick={() => void loadWebSearch()}>
            {t("env.refresh")}
          </Button>
        </div>
        <Text type="secondary">{t("env.webSearchDescription")}</Text>
        <Space style={{ marginTop: 8 }} wrap>
          <Select<CodexWebSearchMode>
            size="small"
            style={{ minWidth: 180 }}
            loading={webSearchSaving}
            placeholder={t("env.webSearchPlaceholder")}
            value={webSearch?.mode}
            onChange={(mode) => void onChangeWebSearch(mode)}
            options={[
              { value: "disabled", label: t("env.webSearchDisabled") },
              { value: "cached", label: t("env.webSearchCached") },
              { value: "indexed", label: t("env.webSearchIndexed") },
              { value: "live", label: t("env.webSearchLive") },
            ]}
          />
          {webSearch ? (
            <Text type="secondary">
              {webSearch.setInConfig
                ? t("env.webSearchConfigPath", { path: webSearch.configPath })
                : t("env.webSearchUnset")}
            </Text>
          ) : null}
        </Space>
      </div>
      <div>
        <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center" }}>
          <Text strong>{t("env.outputProfileTitle")}</Text>
          <Button size="small" onClick={() => void loadOutputProfile()}>
            {t("env.refresh")}
          </Button>
        </div>
        <Text type="secondary">{t("env.outputProfileDescription")}</Text>
        <Space style={{ marginTop: 8 }} direction="vertical" size="small">
          <Space wrap>
            <Select<CodexOutputProfile>
              size="small"
              style={{ minWidth: 180 }}
              loading={outputProfileSaving}
              placeholder={t("env.outputProfilePlaceholder")}
              value={outputProfile?.profile}
              onChange={(profile) =>
                void onChangeOutputProfile(profile, outputProfile?.showRawReasoning ?? false)
              }
              options={[
                { value: "default", label: t("env.outputProfileDefault") },
                { value: "concise", label: t("env.outputProfileConcise") },
                { value: "full", label: t("env.outputProfileFull") },
              ]}
            />
            {outputProfile ? (
              <Text type="secondary">
                {outputProfile.setInConfig
                  ? t("env.outputProfileConfigPath", { path: outputProfile.configPath })
                  : t("env.outputProfileUnset")}
              </Text>
            ) : null}
          </Space>
          <Checkbox
            disabled={outputProfileSaving || !outputProfile || outputProfile.profile === "concise"}
            checked={outputProfile?.profile !== "concise" && Boolean(outputProfile?.showRawReasoning)}
            onChange={(event) => {
              if (!outputProfile) return;
              void onChangeOutputProfile(outputProfile.profile, event.target.checked);
            }}
          >
            {t("env.outputProfileShowRaw")}
          </Checkbox>
        </Space>
      </div>
    </Space>
  );
}
