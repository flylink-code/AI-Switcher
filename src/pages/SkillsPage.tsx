import { useEffect, useState } from "react";
import {
  Button,
  Card,
  Empty,
  Input,
  Modal,
  Space,
  Switch,
  Tag,
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
import { OnboardingTip } from "@/components/OnboardingTip";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import type { RepositorySkill, Skill, SkillTarget, SkillUpdateStatus, UnmanagedSkill } from "@/types/backend";
import {
  deleteSkill,
  checkSkillUpdate,
  checkSkillUpdates,
  ignoreUnmanagedSkill,
  installGithubRepositorySkills,
  installZipSkill,
  refreshGithubRepositorySkills,
  registerUnmanagedSkill,
  setSkillRepository,
  setSkillEnabled,
  updateGithubSkills,
} from "@/services/api";
import {
  skillRepositoryOptions,
  skillsOptions,
  unmanagedSkillsOptions,
} from "@/lib/appQueries";

const { Text } = Typography;
const DEFAULT_SKILL_REPOSITORY = "https://github.com/anthropics/skills";
const SKILLS_TARGET_KEY = "cs.skillsTarget";
const DISCOVERY_PAGE_SIZE = 5;
const INSTALLED_PAGE_SIZE = 8;

function readSkillsTarget(): SkillTarget {
  if (typeof localStorage === "undefined") return "claude_code";
  const stored = localStorage.getItem(SKILLS_TARGET_KEY);
  if (stored === "codex" || stored === "claude_code") return stored;
  return "claude_code";
}

export default function SkillsPage() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [target, setTarget] = useState<SkillTarget>(readSkillsTarget);
  const skillsQuery = useQuery(skillsOptions(target));
  const unmanagedQuery = useQuery(unmanagedSkillsOptions(target));
  const repositoryQuery = useQuery(skillRepositoryOptions);
  const skills = skillsQuery.data ?? [];
  const unmanagedSkills = unmanagedQuery.data ?? [];
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [checkingSkill, setCheckingSkill] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [zipOpen, setZipOpen] = useState(false);
  const [repositoryOpen, setRepositoryOpen] = useState(false);
  const [zipPath, setZipPath] = useState("");
  const [repositoryUrl, setRepositoryUrl] = useState("");
  const [repositorySkills, setRepositorySkills] = useState<RepositorySkill[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [updateStatuses, setUpdateStatuses] = useState<Record<string, SkillUpdateStatus>>({});
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [discoveryBusy, setDiscoveryBusy] = useState<string | null>(null);

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(SKILLS_TARGET_KEY, target);
    }
    setUpdateStatuses({});
  }, [target]);

  useEffect(() => {
    if (!repositoryQuery.data) return;
    setRepositoryUrl(repositoryQuery.data.repositoryUrl);
    setRepositorySkills(repositoryQuery.data.skills);
  }, [repositoryQuery.data]);

  const scanRepository = async () => {
    if (!repositoryUrl.trim()) return;
    setScanning(true);
    try {
      const snapshot = await refreshGithubRepositorySkills(repositoryUrl);
      queryClient.setQueryData(skillRepositoryOptions.queryKey, snapshot);
      setRepositoryUrl(snapshot.repositoryUrl);
      setRepositorySkills(snapshot.skills);
      setSelectedPaths([]);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setScanning(false);
    }
  };

  const restoreDefaultRepository = async () => {
    setScanning(true);
    try {
      const repositoryUrl = await setSkillRepository(DEFAULT_SKILL_REPOSITORY);
      const snapshot = { repositoryUrl, fetchedAt: null, revision: null, skills: [] };
      queryClient.setQueryData(skillRepositoryOptions.queryKey, snapshot);
      setRepositoryUrl(repositoryUrl);
      setRepositorySkills([]);
      setSelectedPaths([]);
      void message.success(t("skills.defaultRepositoryRestored"));
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
      const installed = await installGithubRepositorySkills(repositoryUrl, selectedPaths, target);
      void message.success(t("skills.installedCount", { count: installed.length }));
      setSelectedPaths([]);
      await queryClient.invalidateQueries({ queryKey: ["skills", target] });
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
      await installZipSkill(zipPath, target);
      setZipOpen(false);
      setZipPath("");
      void message.success(t("skills.installed"));
      await queryClient.invalidateQueries({ queryKey: ["skills", target] });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const toggle = async (skill: Skill, enabled: boolean) => {
    setBusy(true);
    try {
      await setSkillEnabled(skill.name, enabled, target);
      queryClient.setQueryData<Skill[]>(["skills", target], (current = []) =>
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
      await deleteSkill(skill.name, target);
      void message.success(t("skills.deleted"));
      await queryClient.invalidateQueries({ queryKey: ["skills", target] });
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
      const status = await checkSkillUpdate(skill.name, target);
      setUpdateStatuses((current) => ({ ...current, [skill.name]: status }));
      void message.info(status.message);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setCheckingSkill(null);
    }
  };

  const checkAllUpdates = async () => {
    setCheckingUpdates(true);
    try {
      const statuses = await checkSkillUpdates(target);
      setUpdateStatuses(Object.fromEntries(statuses.map((status) => [status.name, status])));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setCheckingUpdates(false);
    }
  };

  const applyUpdates = async (names: string[]) => {
    if (!names.length) return;
    setBusy(true);
    try {
      const updated = await updateGithubSkills(names, target);
      void message.success(t("skills.updatedCount", { count: updated.length }));
      await queryClient.invalidateQueries({ queryKey: ["skills", target] });
      setUpdateStatuses((current) => Object.fromEntries(
        Object.entries(current).map(([name, status]) => [name, names.includes(name) ? { ...status, status: "up_to_date" } : status]),
      ));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const confirmUpdates = (names: string[]) => {
    Modal.confirm({
      title: t("skills.confirmUpdateTitle", { count: names.length }),
      content: t("skills.confirmUpdateContent"),
      okText: t("skills.updateSelected", { count: names.length }),
      cancelText: t("providers.cancel"),
      onOk: () => applyUpdates(names),
    });
  };

  const updateAvailableNames = skills.filter((skill) => updateStatuses[skill.name]?.status === "update_available").map((skill) => skill.name);

  const discoverUnmanaged = async () => {
    try {
      const found = await unmanagedQuery.refetch();
      if (found.error) throw found.error;
      void message.info(t("skills.discoveryFound", { count: found.data?.length ?? 0 }));
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const registerUnmanaged = async (skill: UnmanagedSkill) => {
    setDiscoveryBusy(skill.path);
    try {
      await registerUnmanagedSkill(skill.path, target);
      void message.success(t("skills.discoveryRegistered", { name: skill.directory }));
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", target] }),
        queryClient.invalidateQueries({ queryKey: ["unmanagedSkills", target] }),
      ]);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setDiscoveryBusy(null);
    }
  };

  const ignoreUnmanaged = async (skill: UnmanagedSkill) => {
    setDiscoveryBusy(skill.path);
    try {
      await ignoreUnmanagedSkill(skill.path);
      void message.success(t("skills.discoveryIgnored", { name: skill.directory }));
      await queryClient.invalidateQueries({ queryKey: ["unmanagedSkills", target] });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setDiscoveryBusy(null);
    }
  };

  return <Space direction="vertical" size="middle" style={{ width: "100%" }}>
    <OnboardingTip tipKey="skills" message={t("skills.title")} description={t("skills.description")} />
    <WorkspaceTargetSegmented<SkillTarget>
      value={target}
      onChange={setTarget}
      t={t}
      targets={["claude_code", "codex"]}
    />
    <Card
      size="small"
      className="page-surface"
      title={t("skills.discoveryTitle")}
      extra={
        <Button
          icon={<ReloadOutlined />}
          loading={unmanagedQuery.isFetching}
          disabled={busy || scanning}
          onClick={() => void discoverUnmanaged()}
        >
          {t("skills.discoveryScan")}
        </Button>
      }
    >
      <Text type="secondary">{t("skills.discoveryHelp")}</Text>
      <Table
        size="small"
        style={{ marginTop: 8 }}
        rowKey="path"
        dataSource={unmanagedSkills}
        loading={unmanagedQuery.isLoading}
        pagination={{
          pageSize: DISCOVERY_PAGE_SIZE,
          showSizeChanger: true,
          pageSizeOptions: ["5", "10", "20"],
          showTotal: (total) => t("skills.tableTotal", { total }),
          hideOnSinglePage: false,
        }}
        locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("skills.discoveryEmpty")} /> }}
        columns={[
          { title: t("skills.name"), dataIndex: "directory", render: (name: string) => <Text strong>{name}</Text> },
          { title: t("skills.descriptionLabel"), dataIndex: "description", render: (value: string) => value || <Text type="secondary">—</Text> },
          {
            title: t("skills.discoveryFoundIn"),
            dataIndex: "foundIn",
            render: (foundIn: string[]) => foundIn.map((label) => <Tag key={label}>{label}</Tag>),
          },
          {
            title: t("skills.path"),
            dataIndex: "path",
            ellipsis: true,
            render: (path: string) => <Text copyable={{ text: path }} ellipsis style={{ maxWidth: 280 }}>{path}</Text>,
          },
          {
            title: t("skills.actions"),
            width: 180,
            render: (_: unknown, skill: UnmanagedSkill) => (
              <Space size="small">
                <Button
                  type="link"
                  loading={discoveryBusy === skill.path}
                  disabled={busy || scanning || discoveryBusy !== null}
                  onClick={() => void registerUnmanaged(skill)}
                >
                  {t("skills.discoveryRegister")}
                </Button>
                <Button
                  type="link"
                  disabled={busy || scanning || discoveryBusy !== null}
                  onClick={() => void ignoreUnmanaged(skill)}
                >
                  {t("skills.discoveryIgnore")}
                </Button>
              </Space>
            ),
          },
        ]}
      />
    </Card>
    <Card
      size="small"
      className="page-surface"
      title={t("skills.installedTitle")}
      extra={<Space>
        <Button icon={<GithubOutlined />} disabled={busy || scanning} onClick={() => setRepositoryOpen(true)}>{t("skills.repositoryTitle")}</Button>
        <Button loading={checkingUpdates} disabled={busy || scanning} onClick={() => void checkAllUpdates()}>{t("skills.checkAllUpdates")}</Button>
        <Button type="primary" disabled={!updateAvailableNames.length || busy || scanning} onClick={() => confirmUpdates(updateAvailableNames)}>{t("skills.updateSelected", { count: updateAvailableNames.length })}</Button>
        <Button icon={<ReloadOutlined />} loading={refreshing} onClick={() => void refreshSkills()}>{t("common.refresh")}</Button>
        <Button icon={<InboxOutlined />} onClick={() => setZipOpen(true)} disabled={busy || scanning}>{t("skills.installZip")}</Button>
      </Space>}
    >
      <Table
        size="small"
        rowKey="name"
        dataSource={skills}
        loading={skillsQuery.isPending}
        pagination={{
          pageSize: INSTALLED_PAGE_SIZE,
          showSizeChanger: true,
          pageSizeOptions: ["8", "15", "30"],
          showTotal: (total) => t("skills.tableTotal", { total }),
          hideOnSinglePage: false,
        }}
        locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("skills.empty")} /> }}
        columns={[
          { title: t("skills.name"), dataIndex: "name", render: (name: string) => <Text strong>{name}</Text> },
          { title: t("skills.descriptionLabel"), render: (_: unknown, skill: Skill) => (i18n.language === "zh-CN" ? skill.descriptionZh ?? skill.description : skill.description) || <Text type="secondary">—</Text> },
          { title: t("skills.source"), render: (_: unknown, skill: Skill) => skill.source?.sourceUrl ? <Text copyable={{ text: skill.source.sourceUrl }} ellipsis style={{ maxWidth: 220 }}>{skill.source.sourceUrl}</Text> : <Text type="secondary">—</Text> },
          { title: t("skills.enabled"), render: (_: unknown, skill: Skill) => <Switch checked={skill.enabled} disabled={busy || scanning} onChange={(checked) => void toggle(skill, checked)} /> },
          { title: t("skills.updateStatus"), render: (_: unknown, skill: Skill) => <SkillStatus status={updateStatuses[skill.name]} t={t} /> },
          { title: t("skills.actions"), render: (_: unknown, skill: Skill) => <Space size="small"><Button type="link" loading={checkingSkill === skill.name} disabled={busy || scanning || !skill.source} onClick={() => void checkUpdate(skill)}>{t("skills.checkUpdate")}</Button>{updateStatuses[skill.name]?.status === "update_available" && <Button type="link" disabled={busy || scanning} onClick={() => confirmUpdates([skill.name])}>{t("skills.updateSelected", { count: 1 })}</Button>}<Button danger type="link" icon={<DeleteOutlined />} disabled={busy || scanning} onClick={() => void remove(skill)}>{t("skills.delete")}</Button></Space> },
        ]}
      />
    </Card>
    <Modal
      title={t("skills.repositoryTitle")}
      open={repositoryOpen}
      width={840}
      footer={null}
      onCancel={() => setRepositoryOpen(false)}
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
        <Text type="secondary">{t("skills.savedRepository", { repository: repositoryQuery.data?.repositoryUrl ?? DEFAULT_SKILL_REPOSITORY })}</Text>
        {repositoryQuery.data?.fetchedAt && <Text type="secondary">{t("skills.repositoryLastUpdated", { time: new Date(repositoryQuery.data.fetchedAt).toLocaleString() })}</Text>}
        <Space wrap>
          <Button disabled={scanning || busy} onClick={() => void restoreDefaultRepository()}>{t("skills.restoreDefaultRepository")}</Button>
          <Button icon={<ReloadOutlined />} loading={scanning} disabled={!repositoryUrl.trim() || busy} onClick={() => void scanRepository()}>
            {t("skills.scanRepository")}
          </Button>
          <Button type="primary" icon={<DownloadOutlined />} loading={busy} disabled={!selectedPaths.length || scanning} onClick={() => void installSelected()}>
            {t("skills.installSelected", { count: selectedPaths.length })}
          </Button>
        </Space>
        <Table
          size="small"
          rowKey="path"
          dataSource={repositorySkills}
          loading={scanning}
          pagination={{
            pageSize: 10,
            showSizeChanger: true,
            pageSizeOptions: ["10", "20", "50"],
            showTotal: (total) => t("skills.tableTotal", { total }),
            hideOnSinglePage: false,
          }}
          rowSelection={{
            selectedRowKeys: selectedPaths,
            onChange: (keys) => setSelectedPaths(keys.map(String)),
          }}
          locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("skills.repositoryEmpty")} /> }}
          columns={[
            { title: t("skills.name"), dataIndex: "name", render: (name: string) => <Text strong>{name}</Text> },
            { title: t("skills.path"), dataIndex: "path", render: (path: string) => path || <Text type="secondary">/</Text> },
            { title: t("skills.descriptionLabel"), dataIndex: "description", render: (value: string) => value || <Text type="secondary">—</Text> },
          ]}
        />
      </Space>
    </Modal>
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

function SkillStatus({ status, t }: { status?: SkillUpdateStatus; t: (key: string) => string }) {
  if (!status) return <Text type="secondary">—</Text>;
  const color = status.status === "update_available" ? "orange" : status.status === "up_to_date" ? "green" : "default";
  const label = status.status === "update_available" ? t("skills.updateAvailable") : status.status === "up_to_date" ? t("skills.upToDate") : status.message;
  return <Text title={status.message}><Tag color={color}>{label}</Tag></Text>;
}
