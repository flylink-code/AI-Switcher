import { useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Empty,
  Form,
  Input,
  List,
  Modal,
  Popconfirm,
  Space,
  Tooltip,
  Typography,
  theme,
} from "antd";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import FileAddOutlined from "@ant-design/icons/es/icons/FileAddOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import SaveOutlined from "@ant-design/icons/es/icons/SaveOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { OnboardingTip } from "@/components/OnboardingTip";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import type { PromptDetail, PromptInfo, PromptTarget } from "@/types/backend";
import {
  activatePrompt,
  deletePrompt,
  importLivePrompt,
  readPrompt,
  renamePrompt,
  savePrompt,
} from "@/services/api";
import { promptsOverviewOptions } from "@/lib/appQueries";

const { Text, Paragraph } = Typography;

interface PromptFormValues {
  name: string;
  content: string;
}

function promptLiveMeta(target: PromptTarget): { file: string; path: string } {
  switch (target) {
    case "claude_code":
      return { file: "CLAUDE.md", path: "~/.claude/CLAUDE.md" };
    case "codex":
      return { file: "AGENTS.md", path: "~/.codex/AGENTS.md" };
    case "opencode":
      return { file: "AGENTS.md", path: "~/.config/opencode/AGENTS.md" };
    case "pi":
      return { file: "AGENTS.md", path: "~/.pi/agent/AGENTS.md" };
    default: {
      const _exhaustive: never = target;
      return _exhaustive;
    }
  }
}

export default function PromptsPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { token } = theme.useToken();
  const queryClient = useQueryClient();
  const [target, setTarget] = useState<PromptTarget>("claude_code");
  const promptsQuery = useQuery(promptsOverviewOptions(target));
  const prompts = promptsQuery.data?.items ?? [];
  const live = promptsQuery.data?.livePrompt ?? null;
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [editing, setEditing] = useState<PromptDetail | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [form] = Form.useForm<PromptFormValues>();
  const [importForm] = Form.useForm<{ name: string }>();
  const liveMeta = promptLiveMeta(target);

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({
      name: "",
      content: target === "claude_code" ? "# Project Instructions\n\n" : "# AGENTS.md\n\n",
    });
    setFormOpen(true);
  };

  const openEdit = async (info: PromptInfo) => {
    setBusy(true);
    try {
      const detail = await readPrompt(info.name, target);
      setEditing(detail);
      form.setFieldsValue({ name: detail.name, content: detail.content });
      setFormOpen(true);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleSave = async (values: PromptFormValues) => {
    const name = values.name.trim();
    setBusy(true);
    try {
      if (editing && name !== editing.name) {
        await renamePrompt(editing.name, name, target);
        if (values.content !== editing.content) {
          await savePrompt(name, values.content, target);
        }
        void message.success(t("prompts.renamed"));
      } else {
        await savePrompt(name, values.content, target);
        void message.success(t(editing ? "prompts.updated" : "prompts.created"));
      }
      setFormOpen(false);
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions(target).queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleActivate = async (info: PromptInfo) => {
    setBusy(true);
    try {
      await activatePrompt(info.name, target);
      void message.success(t("prompts.activated", { name: info.name, file: liveMeta.file }));
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions(target).queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (info: PromptInfo) => {
    setBusy(true);
    try {
      await deletePrompt(info.name, target);
      void message.success(t("prompts.deleted"));
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions(target).queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const openImportLive = () => {
    importForm.setFieldsValue({ name: "live" });
    setImportOpen(true);
  };

  const handleImportLive = async ({ name }: { name: string }) => {
    setBusy(true);
    try {
      await importLivePrompt(name.trim(), target);
      void message.success(t("prompts.imported", { file: liveMeta.file }));
      setImportOpen(false);
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions(target).queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await promptsQuery.refetch();
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        {promptsQuery.error && <Alert type="error" showIcon message={errMsg(promptsQuery.error)} />}
        <OnboardingTip
          tipKey="prompts"
          message={t("prompts.title", { file: liveMeta.file })}
          description={t("prompts.description", { file: liveMeta.file })}
        />
        <WorkspaceTargetSegmented<PromptTarget>
          value={target}
          onChange={setTarget}
          t={t}
          targets={["claude_code", "codex", "opencode", "pi"]}
        />

        <Card
          size="small"
          className="page-surface"
          title={t("prompts.liveTitle", { file: liveMeta.file })}
          extra={
            <Button
              icon={<ImportOutlined />}
              disabled={!live || busy}
              loading={busy}
              onClick={openImportLive}
            >
              {t("prompts.importLive")}
            </Button>
          }
        >
          {live ? (
            <Space direction="vertical" size={4} style={{ width: "100%" }}>
              <Space>
                <CheckCircleOutlined style={{ color: token.colorSuccess }} />
                <Text strong>{t("prompts.liveDetected")}</Text>
              </Space>
              <Text type="secondary" ellipsis={{ tooltip: live.path }}>{live.path}</Text>
              <Paragraph
                ellipsis={{ rows: 3, expandable: true, symbol: t("prompts.expand") }}
                style={{ marginBottom: 0, whiteSpace: "pre-wrap" }}
              >
                {live.content}
              </Paragraph>
            </Space>
          ) : (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t("prompts.liveMissing", { path: liveMeta.path })}
            />
          )}
        </Card>

        <Card
          size="small"
          className="page-surface"
          title={t("prompts.presetsTitle")}
          extra={
            <Space>
              <Button
                icon={<ReloadOutlined />}
                disabled={busy}
                loading={refreshing}
                onClick={() => void handleRefresh()}
              >
                {t("common.refresh")}
              </Button>
              <Button type="primary" icon={<FileAddOutlined />} disabled={busy} onClick={openCreate}>
                {t("prompts.create")}
              </Button>
            </Space>
          }
        >
          <List
            loading={promptsQuery.isPending}
            dataSource={prompts}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("prompts.empty")} /> }}
            renderItem={(item) => (
              <List.Item
                actions={[
                  <Tooltip key="edit" title={t("prompts.edit")}>
                    <Button size="small" icon={<EditOutlined />} disabled={busy} onClick={() => void openEdit(item)} />
                  </Tooltip>,
                  <Button
                    key="activate"
                    size="small"
                    type="primary"
                    icon={<PlayCircleOutlined />}
                    disabled={busy}
                    onClick={() => void handleActivate(item)}
                  >
                    {t("prompts.activate")}
                  </Button>,
                  <Popconfirm
                    key="delete"
                    title={t("prompts.confirmDelete")}
                    okText={t("prompts.delete")}
                    cancelText={t("common.cancel")}
                    onConfirm={() => void handleDelete(item)}
                    disabled={busy}
                  >
                    <Tooltip title={t("prompts.delete")}>
                      <Button size="small" danger icon={<DeleteOutlined />} disabled={busy} />
                    </Tooltip>
                  </Popconfirm>,
                ]}
              >
                <List.Item.Meta
                  title={item.name}
                  description={t("prompts.presetDescription", { path: liveMeta.path })}
                />
              </List.Item>
            )}
          />
        </Card>
      </Space>

      <Modal
        title={t(editing ? "prompts.editTitle" : "prompts.createTitle")}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => void form.submit()}
        confirmLoading={busy}
        okText={t("prompts.save")}
        cancelText={t("common.cancel")}
        width={780}
      >
        <Form form={form} layout="vertical" onFinish={handleSave}>
          <Form.Item
            name="name"
            label={t("prompts.fieldName")}
            rules={[{ required: true, message: t("prompts.requiredName") }]}
          >
            <Input autoFocus disabled={busy || Boolean(editing)} />
          </Form.Item>
          <Form.Item
            name="content"
            label={t("prompts.fieldContent")}
            rules={[{ required: true, message: t("prompts.requiredContent") }]}
          >
            <Input.TextArea rows={18} disabled={busy} spellCheck={false} style={{ fontFamily: "monospace" }} />
          </Form.Item>
          <Paragraph type="secondary" style={{ marginBottom: 0 }}>
            <SaveOutlined /> {t("prompts.activationNote", { path: liveMeta.path })}
          </Paragraph>
        </Form>
      </Modal>

      <Modal
        title={t("prompts.importLive")}
        open={importOpen}
        onCancel={() => setImportOpen(false)}
        onOk={() => void importForm.submit()}
        confirmLoading={busy}
        okText={t("prompts.importLive")}
        cancelText={t("common.cancel")}
      >
        <Form form={importForm} layout="vertical" onFinish={handleImportLive}>
          <Form.Item
            name="name"
            label={t("prompts.fieldName")}
            rules={[{ required: true, message: t("prompts.requiredName") }]}
          >
            <Input autoFocus disabled={busy} />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
