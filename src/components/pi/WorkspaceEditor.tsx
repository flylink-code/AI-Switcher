import React, { useEffect, useState } from "react";
import { Card, Tabs, Input, Button, Space, Typography, Alert, message, Select } from "antd";
import { SaveOutlined, ReloadOutlined, FileTextOutlined, GlobalOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";

const { Text } = Typography;

export const PROMPT_TEMPLATES = [
  {
    name: "Standard Code Assistant (通用编程助手)",
    content: `# Global Coding Rules

- Always write clear, maintainable, and clean code.
- Prefer explicit error handling and TypeScript types.
- Follow existing patterns in the codebase.
`,
  },
  {
    name: "Refactoring Expert (重构专家)",
    content: `# System Prompt: Refactoring Specialist

- Prioritize code cleanliness, readability, and performance.
- Keep backward compatibility unless explicitly breaking.
`,
  },
];

export const WorkspaceEditor: React.FC = () => {
  const [globalAgentsMd, setGlobalAgentsMd] = useState<string>("");
  const [globalLoading, setGlobalLoading] = useState<boolean>(false);
  const [globalSaving, setGlobalSaving] = useState<boolean>(false);

  const [workspaceDir, setWorkspaceDir] = useState<string>("");
  const [workspaceFileName, setWorkspaceFileName] = useState<string>("AGENTS.md");
  const [workspacePrompt, setWorkspacePrompt] = useState<string>("");
  const [wsLoading, setWsLoading] = useState<boolean>(false);
  const [wsSaving, setWsSaving] = useState<boolean>(false);

  const fetchGlobalAgents = async () => {
    setGlobalLoading(true);
    try {
      const res = await invoke<string>("get_global_pi_agents_md");
      setGlobalAgentsMd(res || "");
    } catch (e: any) {
      message.error(`读取全局 AGENTS.md 失败: ${e.message || e}`);
    } finally {
      setGlobalLoading(false);
    }
  };

  const saveGlobalAgents = async () => {
    setGlobalSaving(true);
    try {
      await invoke("save_global_pi_agents_md", { content: globalAgentsMd });
      message.success("全局 AGENTS.md 保存成功");
    } catch (e: any) {
      message.error(`保存全局 AGENTS.md 失败: ${e.message || e}`);
    } finally {
      setGlobalSaving(false);
    }
  };

  const handleLoadWorkspace = async () => {
    if (!workspaceDir.trim()) {
      message.warning("请输入工作区绝对路径");
      return;
    }
    setWsLoading(true);
    try {
      const res = await invoke<[string, string] | null>("get_workspace_pi_prompt", {
        workspaceDir: workspaceDir.trim(),
      });
      if (res) {
        setWorkspaceFileName(res[0]);
        setWorkspacePrompt(res[1]);
        message.success(`已成功加载工作区 ${res[0]}`);
      } else {
        setWorkspacePrompt("");
        message.info("该工作区下尚未创建 AGENTS.md / SYSTEM.md");
      }
    } catch (e: any) {
      message.error(`加载工作区 Prompt 失败: ${e.message || e}`);
    } finally {
      setWsLoading(false);
    }
  };

  const handleSaveWorkspace = async () => {
    if (!workspaceDir.trim()) {
      message.warning("请输入工作区绝对路径");
      return;
    }
    setWsSaving(true);
    try {
      await invoke("save_workspace_pi_prompt", {
        workspaceDir: workspaceDir.trim(),
        fileName: workspaceFileName,
        content: workspacePrompt,
      });
      message.success(`工作区 ${workspaceFileName} 保存成功`);
    } catch (e: any) {
      message.error(`保存工作区 Prompt 失败: ${e.message || e}`);
    } finally {
      setWsSaving(false);
    }
  };

  useEffect(() => {
    fetchGlobalAgents();
  }, []);

  return (
    <Card title="Pi System Prompt 与工作区指令管理">
      <Tabs
        defaultActiveKey="global"
        items={[
          {
            key: "global",
            label: (
              <Space>
                <GlobalOutlined />
                <span>全局 AGENTS.md (`~/.pi/agent/AGENTS.md`)</span>
              </Space>
            ),
            children: (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <Alert
                  type="info"
                  message="全局系统提示词会在 Pi 所有会话中生效。您可以回填预设模版或自定义修改。"
                  showIcon
                />
                <div style={{ display: "flex", justifyBetween: "space-between", alignItems: "center" }}>
                  <Space>
                    <Text type="secondary">填入常用模版：</Text>
                    <Select
                      style={{ width: 260 }}
                      placeholder="选择并回填模版..."
                      onChange={(val) => setGlobalAgentsMd(val)}
                      options={PROMPT_TEMPLATES.map((t) => ({ label: t.name, value: t.content }))}
                    />
                  </Space>
                  <Space>
                    <Button icon={<ReloadOutlined />} onClick={fetchGlobalAgents} loading={globalLoading}>
                      重新加载
                    </Button>
                    <Button type="primary" icon={<SaveOutlined />} onClick={saveGlobalAgents} loading={globalSaving}>
                      保存全局 AGENTS.md
                    </Button>
                  </Space>
                </div>
                <Input.TextArea
                  rows={12}
                  value={globalAgentsMd}
                  onChange={(e) => setGlobalAgentsMd(e.target.value)}
                  placeholder="# System Prompt..."
                  style={{ fontFamily: "monospace" }}
                />
              </div>
            ),
          },
          {
            key: "workspace",
            label: (
              <Space>
                <FileTextOutlined />
                <span>项目工作区 Prompt Editor</span>
              </Space>
            ),
            children: (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <Space style={{ width: "100%" }}>
                  <Input
                    placeholder="输入工程根目录路径 (如 E:\my-project)"
                    value={workspaceDir}
                    onChange={(e) => setWorkspaceDir(e.target.value)}
                    style={{ width: 420 }}
                  />
                  <Button type="primary" ghost onClick={handleLoadWorkspace} loading={wsLoading}>
                    识别并加载
                  </Button>
                </Space>

                {workspaceDir && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 12, marginTop: 8 }}>
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                      <Space>
                        <Text type="secondary">文件名：</Text>
                        <Select
                          value={workspaceFileName}
                          onChange={(val) => setWorkspaceFileName(val)}
                          options={[
                            { label: "AGENTS.md (推荐)", value: "AGENTS.md" },
                            { label: "SYSTEM.md", value: "SYSTEM.md" },
                          ]}
                        />
                      </Space>
                      <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveWorkspace} loading={wsSaving}>
                        保存工作区指令
                      </Button>
                    </div>

                    <Input.TextArea
                      rows={10}
                      value={workspacePrompt}
                      onChange={(e) => setWorkspacePrompt(e.target.value)}
                      placeholder="# Workspace instructions for Pi..."
                      style={{ fontFamily: "monospace" }}
                    />
                  </div>
                )}
              </div>
            ),
          },
        ]}
      />
    </Card>
  );
};
