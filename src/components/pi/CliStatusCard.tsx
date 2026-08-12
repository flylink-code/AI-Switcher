import React, { useEffect, useState } from "react";
import { Card, Button, Tag, Space, Typography, Alert, Spin, message } from "antd";
import { CheckCircleOutlined, ExclamationCircleOutlined, SyncOutlined, DownloadOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";

const { Text, Title } = Typography;

export interface PiCliVersionInfo {
  installed: boolean;
  currentVersion?: string;
  latestVersion?: string;
  executablePath?: string;
  installCommand: string;
  updateCommand: String;
  error?: string;
}

export const CliStatusCard: React.FC = () => {
  const [loading, setLoading] = useState<boolean>(true);
  const [installing, setInstalling] = useState<boolean>(false);
  const [info, setInfo] = useState<PiCliVersionInfo | null>(null);
  const [logs, setLogs] = useState<string | null>(null);

  const fetchStatus = async () => {
    setLoading(true);
    try {
      const res = await invoke<PiCliVersionInfo>("detect_pi_cli");
      setInfo(res);
    } catch (e: any) {
      message.error(`探测 Pi CLI 失败: ${e.message || e}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchStatus();
  }, []);

  const handleInstallOrUpdate = async () => {
    setInstalling(true);
    setLogs(null);
    try {
      const res = await invoke<string>("install_pi_cli");
      setLogs(res);
      message.success("Pi CLI 安装/更新指令已完成");
      await fetchStatus();
    } catch (e: any) {
      setLogs(e.message || String(e));
      message.error("安装/更新失败");
    } finally {
      setInstalling(false);
    }
  };

  return (
    <Card
      title={
        <Space>
          <span>Pi CLI 环境与版本</span>
          {loading ? (
            <Spin size="small" />
          ) : info?.installed ? (
            <Tag icon={<CheckCircleOutlined />} color="success">
              已安装 {info.currentVersion ? `(v${info.currentVersion})` : ""}
            </Tag>
          ) : (
            <Tag icon={<ExclamationCircleOutlined />} color="warning">
              未检测到
            </Tag>
          )}
        </Space>
      }
      extra={
        <Space>
          <Button icon={<SyncOutlined />} onClick={fetchStatus} loading={loading}>
            刷新
          </Button>
          <Button
            type="primary"
            icon={<DownloadOutlined />}
            loading={installing}
            onClick={handleInstallOrUpdate}
          >
            {info?.installed ? "一键更新" : "一键安装"}
          </Button>
        </Space>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <div>
          <Text type="secondary">包名：</Text>
          <Text code>@earendil-works/pi-coding-agent</Text>
        </div>

        {info?.executablePath && (
          <div>
            <Text type="secondary">可执行文件路径：</Text>
            <Text code>{info.executablePath}</Text>
          </div>
        )}

        {info?.error && !info.installed && (
          <Alert type="info" message={info.error} showIcon />
        )}

        {logs && (
          <Alert
            type={logs.includes("成功") ? "success" : "error"}
            message="命令行执行输出"
            description={
              <pre style={{ maxHeight: 200, overflow: "auto", margin: 0, fontSize: 12 }}>
                {logs}
              </pre>
            }
          />
        )}
      </div>
    </Card>
  );
};
