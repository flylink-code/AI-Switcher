import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Input,
  Modal,
  Space,
  Switch,
  Table,
  Typography,
  message,
} from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import DownloadOutlined from "@ant-design/icons/es/icons/DownloadOutlined";
import GithubOutlined from "@ant-design/icons/es/icons/GithubOutlined";
import InboxOutlined from "@ant-design/icons/es/icons/InboxOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { RepositorySkill, Skill } from "@/types/backend";
import {
  deleteSkill,
  checkSkillUpdate,
  installGithubRepositorySkills,
  installZipSkill,
  listGithubRepositorySkills,
  setSkillEnabled,
  setSkillRepository,
} from "@/services/api";
import { skillRepositoryOptions, skillsOptions } from "@/lib/appQueries";

const { Text } = Typography;

export default function SkillsPage() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const skillsQuery = useQuery(skillsOptions);
  const repositoryQuery = useQuery(skillRepositoryOptions);
  const skills = skillsQuery.data ?? [];
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [checkingSkill, setCheckingSkill] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [zipOpen, setZipOpen] = useState(false);
  const [zipPath, setZipPath] = useState("");
  const [repositoryUrl, setRepositoryUrl] = useState("");
  const [repositorySkills, setRepositorySkills] = useState<RepositorySkill[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);

  useEffect(() => {
    if (repositoryQuery.data) setRepositoryUrl(repositoryQuery.data);
  }, [repositoryQuery.data]);

  const scanRepository = async () => {
    if (!repositoryUrl.trim()) return;
    setScanning(true);
    try {
      const savedUrl = await setSkillRepository(repositoryUrl);
      queryClient.setQueryData(skillRepositoryOptions.queryKey, savedUrl);
      setRepositoryUrl(savedUrl);
      setRepositorySkills(await listGithubRepositorySkills(savedUrl));
      setSelectedPaths([]);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setScanning(false);
    }
  };

  const installSelected = async () => {
    if (!selectedPaths.length) return;
    setBusy(true);
    try {
      const installed = await installGithubRepositorySkills(repositoryUrl, selectedPaths);
      void message.success(t("skills.installedCount", { count: installed.length }));
      setSelectedPaths([]);
      await queryClient.invalidateQueries({ queryKey: skillsOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const installZip = async () => {
    if (!zipPath.trim()) return;
    setBusy(true);
    try {
      await installZipSkill(zipPath);
      setZipOpen(false);
      setZipPath("");
      void message.success(t("skills.installed"));
      await queryClient.invalidateQueries({ queryKey: skillsOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const toggle = async (skill: Skill, enabled: boolean) => {
    setBusy(true);
    try {
      await setSkillEnabled(skill.name, enabled);
      queryClient.setQueryData<Skill[]>(skillsOptions.queryKey, (current = []) =>
        current.map((item) => (item.name === skill.name ? { ...item, enabled } : item)),
      );
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (skill: Skill) => {
    setBusy(true);
    try {
      await deleteSkill(skill.name);
      void message.success(t("skills.deleted"));
      await queryClient.invalidateQueries({ queryKey: skillsOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const refreshSkills = async () => {
    setRefreshing(true);
    try {
      await skillsQuery.refetch();
    } finally {
      setRefreshing(false);
    }
  };

  const checkUpdate = async (skill: Skill) => {
    setCheckingSkill(skill.name);
    try {
      const status = await checkSkillUpdate(skill.name);
      void message.info(status.message);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setCheckingSkill(null);
    }
  };

  return <Space direction="vertical" size="middle" style={{ width: "100%" }}>
    <Alert type="info" showIcon message={t("skills.title")} description={t("skills.description")} />
    <Card
      size="small"
      title={t("skills.repositoryTitle")}
      extra={<Space>
        <Button icon={<ReloadOutlined />} loading={scanning} disabled={!repositoryUrl.trim() || busy} onClick={() => void scanRepository()}>
          {t("skills.scanRepository")}
        </Button>
        <Button type="primary" icon={<DownloadOutlined />} loading={busy} disabled={!selectedPaths.length || scanning} onClick={() => void installSelected()}>
          {t("skills.installSelected", { count: selectedPaths.length })}
        </Button>
      </Space>}
    >
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Input
          prefix={<GithubOutlined />}
          placeholder="https://github.com/anthropics/skills"
          value={repositoryUrl}
          onChange={(e) => setRepositoryUrl(e.target.value)}
          onPressEnter={() => void scanRepository()}
        />
        <Text type="secondary">{t("skills.repositoryHelp")}</Text>
        <Table
          size="small"
          rowKey="path"
          dataSource={repositorySkills}
          loading={scanning}
          pagination={{ pageSize: 10, hideOnSinglePage: true }}
          rowSelection={{
            selectedRowKeys: selectedPaths,
            onChange: (keys) => setSelectedPaths(keys.map(String)),
          }}
          locale={{ emptyText: t("skills.repositoryEmpty") }}
          columns={[
            { title: t("skills.name"), dataIndex: "name", render: (name: string) => <Text strong>{name}</Text> },
            { title: t("skills.path"), dataIndex: "path", render: (path: string) => path || <Text type="secondary">/</Text> },
            { title: t("skills.descriptionLabel"), dataIndex: "description", render: (value: string) => value || <Text type="secondary">—</Text> },
          ]}
        />
      </Space>
    </Card>
    <Card
      size="small"
      title={t("skills.installedTitle")}
      extra={<Space>
        <Button icon={<ReloadOutlined />} loading={refreshing} onClick={() => void refreshSkills()}>{t("common.refresh")}</Button>
        <Button icon={<InboxOutlined />} onClick={() => setZipOpen(true)} disabled={busy || scanning}>{t("skills.installZip")}</Button>
      </Space>}
    >
      <Table
        size="small"
        rowKey="name"
        dataSource={skills}
        loading={skillsQuery.isPending}
        pagination={false}
        locale={{ emptyText: t("skills.empty") }}
        columns={[
          { title: t("skills.name"), dataIndex: "name", render: (name: string) => <Text strong>{name}</Text> },
          { title: t("skills.descriptionLabel"), render: (_: unknown, skill: Skill) => (i18n.language === "zh-CN" ? skill.descriptionZh ?? skill.description : skill.description) || <Text type="secondary">—</Text> },
          { title: t("skills.source"), render: (_: unknown, skill: Skill) => skill.source?.sourceUrl ? <Text copyable={{ text: skill.source.sourceUrl }} ellipsis style={{ maxWidth: 220 }}>{skill.source.sourceUrl}</Text> : <Text type="secondary">—</Text> },
          { title: t("skills.enabled"), render: (_: unknown, skill: Skill) => <Switch checked={skill.enabled} disabled={busy || scanning} onChange={(checked) => void toggle(skill, checked)} /> },
          { title: t("skills.actions"), render: (_: unknown, skill: Skill) => <Space size="small"><Button type="link" loading={checkingSkill === skill.name} disabled={busy || scanning || !skill.source} onClick={() => void checkUpdate(skill)}>{t("skills.checkUpdate")}</Button><Button danger type="link" icon={<DeleteOutlined />} disabled={busy || scanning} onClick={() => void remove(skill)}>{t("skills.delete")}</Button></Space> },
        ]}
      />
    </Card>
    <Modal title={t("skills.installZip")} open={zipOpen} confirmLoading={busy} onOk={() => void installZip()} onCancel={() => setZipOpen(false)}>
      <Space direction="vertical" style={{ width: "100%" }}>
        <Text type="secondary">{t("skills.zipHelp")}</Text>
        <Input placeholder="C:\\Downloads\\my-skill.zip" value={zipPath} onChange={(e) => setZipPath(e.target.value)} onPressEnter={() => void installZip()} />
      </Space>
    </Modal>
  </Space>;
}

function errMsg(e: unknown) {
  return e instanceof Error ? e.message : String(e);
}
