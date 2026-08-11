import { useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Checkbox,
  Form,
  Input,
  Modal,
  Popconfirm,
  Space,
  Table,
  Tag,
  Typography,
  type TableColumnsType,
} from "antd";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { Profile, ProfilePayload, ProfileSnapshotScopes } from "@/types/backend";
import { ResourceEmptyState } from "@/components/workspace/ResourceEmptyState";
import {
  applyProfile,
  createProfile,
  deleteProfile,
  getCurrentProfileId,
  listProfiles,
  updateProfile,
} from "@/services/api";

const { Text, Paragraph } = Typography;

interface CreateFormValues {
  name: string;
  claudeCode: boolean;
  claudeDesktop: boolean;
  codex: boolean;
}

function scopeTags(payload: ProfilePayload, t: (key: string) => string): string[] {
  const tags: string[] = [];
  if (payload.claudeCode) tags.push(t("profiles.scopeCode"));
  if (payload.claudeDesktop) tags.push(t("profiles.scopeDesktop"));
  if (payload.codex) tags.push(t("profiles.scopeCodex"));
  return tags;
}

export default function ProfilesPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<Profile | null>(null);
  const [renameName, setRenameName] = useState("");
  const [applyingId, setApplyingId] = useState<string | null>(null);
  const [createForm] = Form.useForm<CreateFormValues>();

  const profilesQuery = useQuery({
    queryKey: ["profiles"],
    queryFn: listProfiles,
  });
  const currentQuery = useQuery({
    queryKey: ["profiles", "current"],
    queryFn: getCurrentProfileId,
  });

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["profiles"] }),
      queryClient.invalidateQueries({ queryKey: ["profiles", "current"] }),
    ]);
  };

  const handleCreate = async (values: CreateFormValues) => {
    if (!values.claudeCode && !values.claudeDesktop && !values.codex) {
      message.warning(t("profiles.selectScope"));
      return;
    }
    const scopes: ProfileSnapshotScopes = {
      claudeCode: values.claudeCode,
      claudeDesktop: values.claudeDesktop,
      codex: values.codex,
    };
    try {
      await createProfile(values.name.trim(), scopes);
      message.success(t("profiles.created"));
      setCreateOpen(false);
      createForm.resetFields();
      await refresh();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleRename = async () => {
    if (!renameTarget) return;
    const name = renameName.trim();
    if (!name) {
      message.warning(t("profiles.nameRequired"));
      return;
    }
    try {
      await updateProfile(renameTarget.id, name);
      message.success(t("profiles.renamed"));
      setRenameTarget(null);
      await refresh();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleApply = async (profile: Profile) => {
    setApplyingId(profile.id);
    try {
      const result = await applyProfile(profile.id, true);
      if (result.warnings.length > 0) {
        message.warning(
          t("profiles.appliedWithWarnings", { count: result.warnings.length }),
        );
      } else {
        message.success(t("profiles.applied", { name: profile.name }));
      }
      await refresh();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setApplyingId(null);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteProfile(id);
      message.success(t("profiles.deleted"));
      await refresh();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const columns: TableColumnsType<Profile> = [
    {
      title: t("profiles.name"),
      dataIndex: "name",
      render: (name: string, record) => (
        <Space>
          {currentQuery.data === record.id && (
            <CheckCircleOutlined style={{ color: "#52c41a" }} />
          )}
          <Text strong={currentQuery.data === record.id}>{name}</Text>
        </Space>
      ),
    },
    {
      title: t("profiles.scopes"),
      key: "scopes",
      render: (_, record) => (
        <Space wrap>
          {scopeTags(record.payload, t).map((tag) => (
            <Tag key={tag}>{tag}</Tag>
          ))}
        </Space>
      ),
    },
    {
      title: t("profiles.actions"),
      key: "actions",
      width: 220,
      render: (_, record) => (
        <Space>
          <Button
            type="primary"
            size="small"
            icon={<PlayCircleOutlined />}
            loading={applyingId === record.id}
            onClick={() => void handleApply(record)}
          >
            {t("profiles.apply")}
          </Button>
          <Button
            size="small"
            icon={<EditOutlined />}
            onClick={() => {
              setRenameTarget(record);
              setRenameName(record.name);
            }}
          />
          <Popconfirm
            title={t("profiles.deleteConfirm")}
            onConfirm={() => void handleDelete(record.id)}
          >
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const openCreate = () => {
    createForm.setFieldsValue({
      name: "",
      claudeCode: true,
      claudeDesktop: false,
      codex: false,
    });
    setCreateOpen(true);
  };

  const profiles = profilesQuery.data ?? [];

  return (
    <div>
      <Card
        className="page-surface"
        title={t("profiles.title")}
        extra={
          <Space>
            <Button icon={<ReloadOutlined />} onClick={() => void refresh()}>
              {t("common.refresh")}
            </Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
              {t("profiles.create")}
            </Button>
          </Space>
        }
      >
        <Paragraph type="secondary">{t("profiles.subtitle")}</Paragraph>
        {profilesQuery.error && (
          <Alert
            type="error"
            showIcon
            message={profilesQuery.error instanceof Error ? profilesQuery.error.message : String(profilesQuery.error)}
            style={{ marginBottom: 16 }}
          />
        )}
        {!profilesQuery.isLoading && !profilesQuery.error && profiles.length === 0 ? (
          <ResourceEmptyState
            title={t("profiles.emptyTitle", { defaultValue: "暂无项目快照" })}
            description={t("profiles.empty")}
            action={
              <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
                {t("profiles.create")}
              </Button>
            }
          />
        ) : (
          <Table
            rowKey="id"
            loading={profilesQuery.isLoading}
            columns={columns}
            dataSource={profiles}
            pagination={false}
            locale={{ emptyText: t("profiles.empty") }}
          />
        )}
      </Card>

      <Modal
        open={createOpen}
        title={t("profiles.createTitle")}
        okText={t("profiles.create")}
        cancelText={t("common.cancel")}
        onCancel={() => setCreateOpen(false)}
        onOk={() => createForm.submit()}
      >
        <Form form={createForm} layout="vertical" onFinish={(values) => void handleCreate(values)}>
          <Form.Item
            name="name"
            label={t("profiles.name")}
            rules={[{ required: true, message: t("profiles.nameRequired") }]}
          >
            <Input placeholder={t("profiles.namePlaceholder")} />
          </Form.Item>
          <Form.Item label={t("profiles.includeScopes")}>
            <Space direction="vertical">
              <Form.Item name="claudeCode" valuePropName="checked" noStyle>
                <Checkbox>{t("profiles.scopeCode")}</Checkbox>
              </Form.Item>
              <Form.Item name="claudeDesktop" valuePropName="checked" noStyle>
                <Checkbox>{t("profiles.scopeDesktop")}</Checkbox>
              </Form.Item>
              <Form.Item name="codex" valuePropName="checked" noStyle>
                <Checkbox>{t("profiles.scopeCodex")}</Checkbox>
              </Form.Item>
            </Space>
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        open={renameTarget !== null}
        title={t("profiles.renameTitle")}
        okText={t("common.save")}
        cancelText={t("common.cancel")}
        onCancel={() => setRenameTarget(null)}
        onOk={() => void handleRename()}
      >
        <Input
          value={renameName}
          onChange={(event) => setRenameName(event.target.value)}
          placeholder={t("profiles.namePlaceholder")}
        />
      </Modal>
    </div>
  );
}
