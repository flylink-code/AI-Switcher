import { useEffect, useState } from "react";
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
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import SaveOutlined from "@ant-design/icons/es/icons/SaveOutlined";
import { open } from "@tauri-apps/plugin-dialog";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { OnboardingTip } from "@/components/OnboardingTip";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { PromptDetail, PromptInfo, PromptTarget } from "@/types/backend";
import {
  activatePrompt,
  deletePiPromptTemplate,
  deletePrompt,
  getWorkspacePiPrompt,
  importLivePrompt,
  listPiPromptTemplates,
  readPiPromptTemplate,
  readPrompt,
  renamePrompt,
  savePiPromptTemplate,
  savePrompt,
  saveWorkspacePiPrompt,
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
    case "cline":
      return { file: "AGENTS.md", path: "~/.cline/rules/AGENTS.md" };
    default: {
      const _exhaustive: never = target;
      return _exhaustive;
    }
  }
}

interface PromptsPageProps {
  target?: PromptTarget;
}

export default function PromptsPage({ target: targetProp }: PromptsPageProps = {}) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { token } = theme.useToken();
  const queryClient = useQueryClient();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const getValidPromptTarget = (preferred: PromptTarget): PromptTarget => {
    const validTargets = visibleAgents.filter((a): a is PromptTarget => a === "claude_code" || a === "codex" || a === "opencode" || a === "pi" || a === "cline");
    if (validTargets.includes(preferred)) return preferred;
    return validTargets[0] ?? "claude_code";
  };

  const [internalTarget, setInternalTarget] = useState<PromptTarget>(() => getValidPromptTarget("claude_code"));
  const target = targetProp ?? internalTarget;

  useEffect(() => {
    if (!targetProp) {
      const valid = getValidPromptTarget(internalTarget);
      if (valid !== internalTarget) {
        setInternalTarget(valid);
      }
    }
  }, [visibleAgents, internalTarget, targetProp]);

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

  // Workspace prompt state
  const [wsDir, setWsDir] = useState<string | null>(null);
  const [wsFile, setWsFile] = useState<string>("AGENTS.md");
  const [wsContent, setWsContent] = useState<string>("");
  const [wsLoading, setWsLoading] = useState(false);
  const [wsSaving, setWsSaving] = useState(false);

  // Pi templates state
  const piTemplatesQuery = useQuery({
    queryKey: ["pi-prompt-templates"],
    queryFn: () => listPiPromptTemplates(),
    enabled: target === "pi",
  });

  const handleSelectWsDir = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: t("prompts.selectWsFolder", { defaultValue: "选择项目工作区目录" }) });
      if (typeof selected === "string") {
        setWsDir(selected);
        setWsLoading(true);
        const res = await getWorkspacePiPrompt(selected);
        if (res) {
          setWsFile(res[0]);
          setWsContent(res[1]);
        } else {
          setWsFile("AGENTS.md");
          setWsContent("# Project AGENTS.md\n\n");
        }
        setWsLoading(false);
      }
    } catch (e) {
      void message.error(errMsg(e));
      setWsLoading(false);
    }
  };

  const handleSaveWsPrompt = async () => {
    if (!wsDir) return;
    setWsSaving(true);
    try {
      await saveWorkspacePiPrompt(wsDir, wsFile, wsContent);
      void message.success(t("prompts.wsPromptSaved", { defaultValue: `项目 ${wsFile} 已成功保存` }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setWsSaving(false);
    }
  };

  const handleCreatePiTemplate = async () => {
    const name = window.prompt(t("prompts.templateNamePrompt", { defaultValue: "请输入模板文件名 (例: code-review.md):" }), "template.md");
    if (!name?.trim()) return;
    try {
      await savePiPromptTemplate(name.trim(), `# ${name}\n\n`);
      void message.success(t("prompts.templateCreated", { defaultValue: "模板创建成功" }));
      void piTemplatesQuery.refetch();
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleEditPiTemplate = async (templateName: string) => {
    try {
      const content = await readPiPromptTemplate(templateName);
      setEditing({ name: templateName, content, updatedAt: Date.now() });
      form.setFieldsValue({ name: templateName, content });
      setFormOpen(true);
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleDeletePiTemplate = async (templateName: string) => {
    try {
      await deletePiPromptTemplate(templateName);
      void message.success(t("prompts.templateDeleted", { defaultValue: "模板已删除" }));
      void piTemplatesQuery.refetch();
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

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
        {!targetProp && (
          <WorkspaceTargetSegmented<PromptTarget>
            value={target}
            onChange={setInternalTarget}
            t={t}
            targets={["claude_code", "codex", "opencode", "pi", "cline"]}
          />
        )}

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
          title={t("prompts.workspaceEditorTitle", { defaultValue: "项目级 Prompt 编辑器 (Workspace AGENTS.md / CLAUDE.md)" })}
          extra={
            <Space>
              <Button icon={<FolderOpenOutlined />} onClick={() => void handleSelectWsDir()}>
                {t("prompts.chooseWsDir", { defaultValue: "选择项目目录" })}
              </Button>
              {wsDir && (
                <Button type="primary" icon={<SaveOutlined />} loading={wsSaving} onClick={() => void handleSaveWsPrompt()}>
                  {t("common.save", { defaultValue: "保存" })}
                </Button>
              )}
            </Space>
          }
        >
          {wsDir ? (
            <Space direction="vertical" style={{ width: "100%" }}>
              <Text type="secondary">项目: {wsDir} ({wsFile})</Text>
              <Input.TextArea
                rows={8}
                value={wsContent}
                disabled={wsLoading}
                onChange={(e) => setWsContent(e.target.value)}
                style={{ fontFamily: "monospace" }}
              />
            </Space>
          ) : (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t("prompts.wsDirNotSelected", { defaultValue: "未选择项目目录。点击右上角「选择项目目录」即可可视化编辑项目的 AGENTS.md / CLAUDE.md / SYSTEM.md。" })}
            />
          )}
        </Card>

        {target === "pi" && (
          <Card
            size="small"
            className="page-surface"
            title={t("prompts.piTemplatesTitle", { defaultValue: "Pi Prompt 模板目录 (~/.pi/agent/prompts/)" })}
            extra={
              <Button icon={<FileAddOutlined />} onClick={() => void handleCreatePiTemplate()}>
                {t("prompts.createTemplate", { defaultValue: "新建模板" })}
              </Button>
            }
          >
            <List
              dataSource={piTemplatesQuery.data ?? []}
              locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("prompts.noTemplates", { defaultValue: "暂无 Pi Prompt 模板" })} /> }}
              renderItem={(tmpl) => (
                <List.Item
                  actions={[
                    <Button key="edit" size="small" icon={<EditOutlined />} onClick={() => void handleEditPiTemplate(tmpl)}>
                      {t("prompts.edit")}
                    </Button>,
                    <Popconfirm key="del" title={t("prompts.confirmDelete")} onConfirm={() => void handleDeletePiTemplate(tmpl)}>
                      <Button size="small" danger icon={<DeleteOutlined />} />
                    </Popconfirm>,
                  ]}
                >
                  <List.Item.Meta title={tmpl} description="~/.pi/agent/prompts/" />
                </List.Item>
              )}
            />
          </Card>
        )}

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
