import { useState } from "react";
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
import GithubOutlined from "@ant-design/icons/es/icons/GithubOutlined";
import InboxOutlined from "@ant-design/icons/es/icons/InboxOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { Skill } from "@/types/backend";
import { deleteSkill, installGithubSkill, installZipSkill, setSkillEnabled } from "@/services/api";
import { skillsOptions } from "@/lib/appQueries";

const { Text } = Typography;

export default function SkillsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const skillsQuery = useQuery(skillsOptions);
  const skills = skillsQuery.data ?? [];
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [githubOpen, setGithubOpen] = useState(false);
  const [zipOpen, setZipOpen] = useState(false);
  const [githubUrl, setGithubUrl] = useState("");
  const [zipPath, setZipPath] = useState("");

  const installGithub = async () => {
    if (!githubUrl.trim()) return;
    setBusy(true);
    try {
      await installGithubSkill(githubUrl);
      setGithubOpen(false);
      setGithubUrl("");
      void message.success(t("skills.installed"));
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

  return <>
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Alert type="info" showIcon message={t("skills.title")} description={t("skills.description")} />
      <Card
        size="small"
        title={t("skills.installedTitle")}
        extra={<Space>
          <Button
            icon={<ReloadOutlined />}
            loading={refreshing}
            onClick={() => void refreshSkills()}
          >
            {t("common.refresh")}
          </Button>
          <Button icon={<InboxOutlined />} onClick={() => setZipOpen(true)} disabled={busy}>{t("skills.installZip")}</Button>
          <Button type="primary" icon={<GithubOutlined />} onClick={() => setGithubOpen(true)}>{t("skills.installGithub")}</Button>
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
            { title: t("skills.descriptionLabel"), dataIndex: "description", render: (value: string) => value || <Text type="secondary">—</Text> },
            { title: t("skills.enabled"), render: (_: unknown, skill: Skill) => <Switch checked={skill.enabled} onChange={(checked) => void toggle(skill, checked)} /> },
            { title: t("skills.actions"), render: (_: unknown, skill: Skill) => <Button danger type="link" icon={<DeleteOutlined />} onClick={() => void remove(skill)}>{t("skills.delete")}</Button> },
          ]}
        />
      </Card>
    </Space>
    <Modal title={t("skills.installZip")} open={zipOpen} confirmLoading={busy} onOk={() => void installZip()} onCancel={() => setZipOpen(false)}>
      <Space direction="vertical" style={{ width: "100%" }}>
        <Text type="secondary">{t("skills.zipHelp")}</Text>
        <Input placeholder="C:\\Downloads\\my-skill.zip" value={zipPath} onChange={(e) => setZipPath(e.target.value)} onPressEnter={() => void installZip()} />
      </Space>
    </Modal>
    <Modal title={t("skills.installGithub")} open={githubOpen} confirmLoading={busy} onOk={() => void installGithub()} onCancel={() => setGithubOpen(false)}>
      <Space direction="vertical" style={{ width: "100%" }}>
        <Text type="secondary">{t("skills.githubHelp")}</Text>
        <Input prefix={<GithubOutlined />} placeholder="https://github.com/owner/repository" value={githubUrl} onChange={(e) => setGithubUrl(e.target.value)} onPressEnter={() => void installGithub()} />
      </Space>
    </Modal>
  </>;
}

function errMsg(e: unknown) { return e instanceof Error ? e.message : String(e); }
