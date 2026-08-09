import { useState } from "react";
import { App as AntApp } from "antd";
import { useTranslation } from "react-i18next";
import { showCodexSwitchNotice } from "@/lib/codexNotice";
import { useProvidersStore } from "@/stores/providersStore";
import type { ImportPreview, Provider, ProviderInput, ProviderTarget } from "@/types/backend";
import {
  buildProviderDeeplink,
  confirmImportPreview,
  exportProviders,
  previewImportText,
  speedtestProviderEndpoint,
  testProviderConnection,
} from "@/services/api";

export function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Shared provider card/row actions used by WorkbenchPage and ProvidersPage.
 * Exposes both the shared `busy` flag (table pages disable whole rows) and the
 * per-action flags `switchingId` / `testingId` / `batchTesting` (card pages spin
 * only the button being clicked — do not collapse these into `busy`).
 */
export function useProviderActions(options: {
  target: ProviderTarget;
  editing: Provider | null;
  closeForm: () => void;
}) {
  const { target, editing, closeForm } = options;
  const { t } = useTranslation();
  const { message } = AntApp.useApp();
  const store = useProvidersStore();

  const [busy, setBusy] = useState(false);
  const [switchingId, setSwitchingId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [batchTesting, setBatchTesting] = useState(false);
  const [importPreview, setImportPreview] = useState<ImportPreview | null>(null);
  const [importConfirming, setImportConfirming] = useState(false);

  const handleSubmit = async (input: ProviderInput) => {
    try {
      if (editing) {
        await store.update(input);
        void message.success(t("providers.updated"));
      } else {
        await store.create(input);
        void message.success(t("providers.created"));
      }
      closeForm();
    } catch (e) {
      void message.error(errMsg(e));
      throw e;
    }
  };

  const handleSwitch = async (provider: Provider) => {
    if (!provider.apiKeySet) {
      void message.warning(t("providers.missingKey"));
      return;
    }
    setSwitchingId(provider.id);
    setBusy(true);
    try {
      const result = await store.switchTo(provider.id);
      void message.success(t("providers.switched", { name: provider.name }));
      void message.info(t("providers.hotSwitchHint"));
      const sync = result.sessionSync;
      if (sync) {
        if (sync.status === "warning") {
          void message.warning(sync.message);
        } else if (sync.changedSessionFiles > 0 || sync.sqliteRowsUpdated > 0) {
          void message.success(
            t("providers.sessionSyncSummary", {
              files: sync.changedSessionFiles,
              rows: sync.sqliteRowsUpdated,
            }),
          );
        }
      }
      showCodexSwitchNotice(result.codexNotice, message, t);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setSwitchingId(null);
      setBusy(false);
    }
  };

  const handleOfficial = async () => {
    setSwitchingId("official");
    setBusy(true);
    try {
      await store.useOfficial();
      void message.success(t("providers.switchedOfficial"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setSwitchingId(null);
      setBusy(false);
    }
  };

  const handleTest = async (provider: Provider) => {
    setTestingId(provider.id);
    setBusy(true);
    try {
      const result = await testProviderConnection(provider.id);
      const notify = result.ok ? message.success : message.error;
      void notify(
        result.latencyMs != null
          ? `${result.message} · ${t("providers.latencyMs", { ms: result.latencyMs })}`
          : result.message,
      );
      useProvidersStore.setState((state) => ({
        providers: state.providers.map((item) =>
          item.id === provider.id
            ? {
                ...item,
                healthStatus: result.ok ? "healthy" : "error",
                healthCheckedAt: result.checkedAt,
                healthLatencyMs: result.latencyMs ?? null,
              }
            : item,
        ),
      }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setTestingId(null);
      setBusy(false);
    }
  };

  const handleSpeedtest = async (provider: Provider) => {
    setBusy(true);
    try {
      const result = await speedtestProviderEndpoint(provider.id);
      const notify = result.ok ? message.success : message.warning;
      void notify(result.message);
      useProvidersStore.setState((state) => ({
        providers: state.providers.map((item) =>
          item.id === provider.id
            ? {
                ...item,
                healthLatencyMs: result.latencyMs ?? item.healthLatencyMs ?? null,
              }
            : item,
        ),
      }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleSpeedtestAll = async () => {
    setBatchTesting(true);
    try {
      let count = 0;
      for (const p of store.providers) {
        try {
          const res = await speedtestProviderEndpoint(p.id);
          if (res.ok) count++;
          useProvidersStore.setState((state) => ({
            providers: state.providers.map((item) =>
              item.id === p.id
                ? {
                    ...item,
                    healthLatencyMs: res.latencyMs ?? item.healthLatencyMs ?? null,
                  }
                : item,
            ),
          }));
        } catch {
          // ignore individual errors during batch test
        }
      }
      void message.success(
        t("providers.speedtestAllDone", {
          defaultValue: `已完成 ${store.providers.length} 个供应商测速（成功 ${count}）`,
          total: store.providers.length,
          ok: count,
        }),
      );
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBatchTesting(false);
    }
  };

  const handleShareLink = async (provider: Provider) => {
    try {
      const link = await buildProviderDeeplink(provider.id);
      await navigator.clipboard.writeText(link);
      void message.success(t("deeplink.linkCopied"));
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleDelete = async (provider: Provider) => {
    setBusy(true);
    try {
      await store.remove(provider.id);
      void message.success(t("providers.deleted"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleExport = async () => {
    try {
      const json = await exportProviders(target);
      const url = URL.createObjectURL(new Blob([json], { type: "application/json" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `claude-switcher-providers-${target}.json`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleImportLive = async () => {
    setBusy(true);
    try {
      await store.importLive();
      void message.success(
        target === "opencode" ? t("providers.syncOpenCodeLiveDone") : t("providers.imported"),
      );
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleImportClipboard = async () => {
    setBusy(true);
    try {
      const text = await navigator.clipboard.readText();
      const preview = await previewImportText(text);
      setImportPreview(preview);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleImportFile = async (file: File) => {
    setBusy(true);
    try {
      const preview = await previewImportText(await file.text());
      setImportPreview(preview);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleConfirmImport = async () => {
    if (!importPreview) return;
    setImportConfirming(true);
    try {
      const result = await confirmImportPreview(importPreview);
      void message.success(
        t("providers.importSummary", { imported: result.imported, skipped: result.skipped }),
      );
      setImportPreview(null);
      await store.load(target);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setImportConfirming(false);
    }
  };

  return {
    busy,
    setBusy,
    switchingId,
    testingId,
    batchTesting,
    importPreview,
    importConfirming,
    setImportPreview,
    handleSubmit,
    handleSwitch,
    handleOfficial,
    handleTest,
    handleSpeedtest,
    handleSpeedtestAll,
    handleShareLink,
    handleDelete,
    handleExport,
    handleImportLive,
    handleImportClipboard,
    handleImportFile,
    handleConfirmImport,
  };
}
