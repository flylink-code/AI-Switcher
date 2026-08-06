import { useState } from "react";
import {
  Button,
  Card,
  Form,
  Input,
  Modal,
  Popconfirm,
  Space,
  Switch,
  Table,
  Typography,
  message,
} from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import InboxOutlined from "@ant-design/icons/es/icons/InboxOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { open } from "@tauri-apps/plugin-dialog";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { OnboardingTip } from "@/components/OnboardingTip";
import type { Agent, AgentDraft } from "@/types/backend";
import {
  deleteAgent,
  installZipAgent,
  saveAgent,
  setAgentEnabled,
} from "@/services/api";
import { agentsOptions } from "@/lib/appQueries";

const { Text, Paragraph } = Typography;

export default function AgentsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const agentsQuery = useQuery(agentsOptions);
  const agents = agentsQuery.data ?? [];
  const [busy, setBusy] = useState(false);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Agent | null>(null);
  const [form] = Form.useForm<AgentDraft>();

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: ["agents"] });
  };

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({ name: "", description: "", body: "" });
    setFormOpen(true);
  };

  const openEdit = (agent: Agent) => {
    setEditing(agent);
    form.setFieldsValue({
      name: agent.name,
      description: agent.description,
      body: "",
    });
    setFormOpen(true);
  };

  const handleSave = async (values: AgentDraft) => {
    setBusy(true);
    try {
      await saveAgent(values);
      void message.success(t("agents.saved"));
      setFormOpen(false);
      await refresh();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const toggleEnabled = async (agent: Agent, enabled: boolean) => {
    setBusy(true);
    try {
      await setAgentEnabled(agent.name, enabled);
      await refresh();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (agent: Agent) => {
    setBusy(true);
    try {
      await deleteAgent(agent.name);
      void message.success(t("agents.deleted"));
      await refresh();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const installZip = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    setBusy(true);
    try {
      const installed = await installZipAgent(selected);
      void message.success(t("agents.installedCount", { count: installed.length }));
      await refresh();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      <OnboardingTip tipKey="agents" type="info" message={t("agents.tip")} />
      <Card
        className="page-surface"
        title={t("agents.title")}
        extra={
          <Space wrap>
            <Button icon={<ReloadOutlined />} loading={agentsQuery.isFetching} onClick={() => void refresh()}>
              {t("common.refresh")}
            </Button>
            <Button icon={<InboxOutlined />} loading={busy} onClick={() => void installZip()}>
              {t("agents.installZip")}
            </Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
              {t("agents.create")}
            </Button>
          </Space>
        }
      >
        <Paragraph type="secondary">{t("agents.subtitle")}</Paragraph>
        <Table
          rowKey="path"
          loading={agentsQuery.isLoading || busy}
          dataSource={agents}
          pagination={false}
          columns={[
            {
              title: t("agents.fieldName"),
              dataIndex: "name",
              render: (name: string, row: Agent) => (
                <Space direction="vertical" size={0}>
                  <Text strong>{name}</Text>
                  <Text type="secondary" ellipsis style={{ maxWidth: 420 }}>
                    {row.description || t("agents.noDescription")}
                  </Text>
                </Space>
              ),
            },
            {
              title: t("agents.enabled"),
              width: 100,
              render: (_: unknown, row: Agent) => (
                <Switch checked={row.enabled} onChange={(checked) => void toggleEnabled(row, checked)} />
              ),
            },
            {
              title: t("agents.actions"),
              width: 160,
              render: (_: unknown, row: Agent) => (
                <Space>
                  <Button size="small" onClick={() => openEdit(row)}>
                    {t("agents.edit")}
                  </Button>
                  <Popconfirm title={t("agents.confirmDelete")} onConfirm={() => void remove(row)}>
                    <Button size="small" danger icon={<DeleteOutlined />} />
                  </Popconfirm>
                </Space>
              ),
            },
          ]}
        />
      </Card>

      <Modal
        open={formOpen}
        title={editing ? t("agents.editTitle") : t("agents.createTitle")}
        confirmLoading={busy}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        destroyOnHidden
      >
        <Form form={form} layout="vertical" onFinish={(values) => void handleSave(values)}>
          <Form.Item
            name="name"
            label={t("agents.fieldName")}
            rules={[{ required: true, message: t("agents.requiredName") }]}
          >
            <Input disabled={Boolean(editing)} placeholder="code-reviewer" />
          </Form.Item>
          <Form.Item
            name="description"
            label={t("agents.fieldDescription")}
            rules={[{ required: true, message: t("agents.requiredDescription") }]}
          >
            <Input.TextArea rows={2} placeholder={t("agents.descriptionPlaceholder")} />
          </Form.Item>
          <Form.Item name="body" label={t("agents.fieldBody")} extra={editing ? t("agents.bodyEditHint") : undefined}>
            <Input.TextArea rows={8} placeholder={t("agents.bodyPlaceholder")} />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
}
