import React, { useEffect, useState } from "react";
import { Card, Table, Input, Button, Space, Typography, Tag, Modal, Spin, message, Tooltip } from "antd";
import { SearchOutlined, ReloadOutlined, CodeOutlined, ExportOutlined, CopyOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";

const { Text, Paragraph } = Typography;

export interface PiSessionItem {
  id: string;
  filePath: string;
  title?: string;
  createdAt?: number;
  updatedAt?: number;
  tokenCount?: number;
  model?: string;
  provider?: string;
}

export const SessionManager: React.FC = () => {
  const [sessions, setSessions] = useState<PiSessionItem[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [searchText, setSearchText] = useState<string>("");

  const [selectedSession, setSelectedSession] = useState<PiSessionItem | null>(null);
  const [detailModalOpen, setDetailModalOpen] = useState<boolean>(false);
  const [detailContent, setDetailContent] = useState<string>("");
  const [detailLoading, setDetailLoading] = useState<boolean>(false);

  const fetchSessions = async () => {
    setLoading(true);
    try {
      const res = await invoke<PiSessionItem[]>("list_pi_sessions");
      setSessions(res || []);
    } catch (e: any) {
      message.error(`读取 Pi 会话列表失败: ${e.message || e}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchSessions();
  }, []);

  const handleOpenDetail = async (item: PiSessionItem) => {
    setSelectedSession(item);
    setDetailModalOpen(true);
    setDetailLoading(true);
    try {
      const text = await invoke<string>("read_pi_session_detail", { filePath: item.filePath });
      setDetailContent(text || "");
    } catch (e: any) {
      message.error(`读取会话详情失败: ${e.message || e}`);
    } finally {
      setDetailLoading(false);
    }
  };

  const copyResumeCmd = (id: string) => {
    const cmd = `pi --resume ${id}`;
    navigator.clipboard.writeText(cmd);
    message.success(`已复制终端恢复指令: ${cmd}`);
  };

  const exportMarkdown = () => {
    if (!selectedSession || !detailContent) return;
    const blob = new Blob([detailContent], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `pi-session-${selectedSession.id}.md`;
    a.click();
    URL.revokeObjectURL(url);
    message.success("会话已导出为 Markdown 文件");
  };

  const filteredSessions = sessions.filter((s) => {
    if (!searchText.trim()) return true;
    const q = searchText.toLowerCase();
    return (
      s.id.toLowerCase().includes(q) ||
      (s.title && s.title.toLowerCase().includes(q)) ||
      (s.model && s.model.toLowerCase().includes(q)) ||
      (s.provider && s.provider.toLowerCase().includes(q))
    );
  });

  const columns = [
    {
      title: "会话 ID / 标题",
      dataIndex: "title",
      key: "title",
      render: (text: string, record: PiSessionItem) => (
        <Space direction="vertical" size={2}>
          <Text bold>{text || record.id}</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>ID: {record.id}</Text>
        </Space>
      ),
    },
    {
      title: "供应商 / 模型",
      key: "provider",
      render: (_: any, record: PiSessionItem) => (
        <Space>
          {record.provider && <Tag color="blue">{record.provider}</Tag>}
          {record.model ? <Text code style={{ fontSize: 12 }}>{record.model}</Text> : <Text type="secondary">-</Text>}
        </Space>
      ),
    },
    {
      title: "Tokens",
      dataIndex: "tokenCount",
      key: "tokenCount",
      render: (tokens?: number) => (tokens ? `${tokens.toLocaleString()} tokens` : "-"),
    },
    {
      title: "最后更新",
      dataIndex: "updatedAt",
      key: "updatedAt",
      render: (ts?: number) => (ts ? new Date(ts * 1000).toLocaleString() : "-"),
    },
    {
      title: "操作",
      key: "action",
      render: (_: any, record: PiSessionItem) => (
        <Space>
          <Button size="small" onClick={() => handleOpenDetail(record)}>
            查看内容
          </Button>
          <Tooltip title={`点击复制 pi --resume ${record.id}`}>
            <Button size="small" icon={<CodeOutlined />} onClick={() => copyResumeCmd(record.id)}>
              恢复会话
            </Button>
          </Tooltip>
        </Space>
      ),
    },
  ];

  return (
    <Card
      title="Pi 会话历史与终端接管"
      extra={
        <Space>
          <Input
            placeholder="搜索会话 ID / 标题 / 模型..."
            prefix={<SearchOutlined />}
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
            style={{ width: 240 }}
          />
          <Button icon={<ReloadOutlined />} onClick={fetchSessions} loading={loading}>
            刷新
          </Button>
        </Space>
      }
    >
      <Table
        rowKey="id"
        columns={columns}
        dataSource={filteredSessions}
        loading={loading}
        pagination={{ pageSize: 8 }}
        size="small"
      />

      <Modal
        title={`会话详情 - ${selectedSession?.id}`}
        open={detailModalOpen}
        onCancel={() => setDetailModalOpen(false)}
        width={720}
        footer={[
          <Button key="copy" icon={<CopyOutlined />} onClick={() => selectedSession && copyResumeCmd(selectedSession.id)}>
            复制恢复命令
          </Button>,
          <Button key="export" icon={<ExportOutlined />} type="primary" onClick={exportMarkdown}>
            导出 Markdown
          </Button>,
        ]}
      >
        {detailLoading ? (
          <div style={{ textAlign: "center", padding: 40 }}><Spin /></div>
        ) : (
          <div style={{ maxHeight: 450, overflow: "auto" }}>
            <pre style={{ margin: 0, fontSize: 12, fontFamily: "monospace", whiteSpace: "pre-wrap" }}>
              {detailContent}
            </pre>
          </div>
        )}
      </Modal>
    </Card>
  );
};
