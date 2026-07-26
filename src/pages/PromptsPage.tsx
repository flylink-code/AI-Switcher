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
import type { PromptDetail, PromptInfo } from "@/types/backend";
import {
  activatePrompt,
  deletePrompt,
  importLivePrompt,
  readPrompt,
  savePrompt,
} from "@/services/api";
import { promptsOverviewOptions } from "@/lib/appQueries";

const { Text, Paragraph } = Typography;

interface PromptFormValues {
  name: string;
  content: string;
}

export default function PromptsPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { token } = theme.useToken();
  const queryClient = useQueryClient();
  const promptsQuery = useQuery(promptsOverviewOptions);
  const prompts = promptsQuery.data?.items ?? [];
  const live = promptsQuery.data?.livePrompt ?? null;
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<PromptDetail | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [form] = Form.useForm<PromptFormValues>();
  const [importForm] = Form.useForm<{ name: string }>();

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({ name: "", content: "# Project Instructions\n\n" });
    setFormOpen(true);
  };

  const openEdit = async (info: PromptInfo) => {
    setBusy(true);
    try {
      const detail = await readPrompt(info.name);
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
    if (editing && name !== editing.name) {
      void message.warning(t("prompts.renameUnsupported"));
      return;
    }
    setBusy(true);
    try {
      await savePrompt(name, values.content);
      void message.success(t(editing ? "prompts.updated" : "prompts.created"));
      setFormOpen(false);
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleActivate = async (info: PromptInfo) => {
    setBusy(true);
    try {
      await activatePrompt(info.name);
      void message.success(t("prompts.activated", { name: info.name }));
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (info: PromptInfo) => {
    setBusy(true);
    try {
      await deletePrompt(info.name);
      void message.success(t("prompts.deleted"));
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions.queryKey });
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
      await importLivePrompt(name.trim());
      void message.success(t("prompts.imported"));
      setImportOpen(false);
      await queryClient.invalidateQueries({ queryKey: promptsOverviewOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        {promptsQuery.error && <Alert type="error" showIcon message={errMsg(promptsQuery.error)} />}
        <Alert type="info" showIcon message={t("prompts.title")} description={t("prompts.description")} />

        <Card
          size="small"
          title={t("prompts.liveTitle")}
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
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("prompts.liveMissing")} />
          )}
        </Card>

        <Card
          size="small"
          title={t("prompts.presetsTitle")}
          extra={
            <Space>
              <Button
                icon={<ReloadOutlined />}
                disabled={busy}
                loading={promptsQuery.isFetching}
                onClick={() => void promptsQuery.refetch()}
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
            locale={{ emptyText: t("prompts.empty") }}
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
                <List.Item.Meta title={item.name} description={t("prompts.presetDescription")} />
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
            <SaveOutlined /> {t("prompts.activationNote")}
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
