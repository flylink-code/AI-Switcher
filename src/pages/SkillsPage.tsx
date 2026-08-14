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
import ArrowLeftOutlined from "@ant-design/icons/es/icons/ArrowLeftOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import DownloadOutlined from "@ant-design/icons/es/icons/DownloadOutlined";
import GithubOutlined from "@ant-design/icons/es/icons/GithubOutlined";
import InboxOutlined from "@ant-design/icons/es/icons/InboxOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import RightOutlined from "@ant-design/icons/es/icons/RightOutlined";
import SearchOutlined from "@ant-design/icons/es/icons/SearchOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { OnboardingTip } from "@/components/OnboardingTip";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { RepositorySkill, Skill, SkillRepositorySnapshot, SkillTarget, SkillUpdateStatus, UnmanagedSkill } from "@/types/backend";
import {
  addSkillRepository,
  deleteSkill,
  buildSkillDeeplink,
  checkSkillUpdate,
  checkSkillUpdates,
  ignoreUnmanagedSkill,
  installGithubRepositorySkills,
  installZipSkill,
  refreshGithubRepositorySkills,
  registerUnmanagedSkill,
  removeSkillRepository,
  setSkillEnabled,
  updateGithubSkills,
} from "@/services/api";
import {
  skillRepositoriesOptions,
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
  if (stored === "codex" || stored === "claude_code" || stored === "pi") return stored;
  return "claude_code";
}

function getTargetLabel(target: SkillTarget): string {
  switch (target) {
    case "claude_code":
      return "Claude Code";
    case "codex":
      return "Codex";
    case "pi":
      return "Pi";
  }
}

interface SkillsPageProps {
  target?: SkillTarget;
}

export default function SkillsPage({ target: targetProp }: SkillsPageProps = {}) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const getValidSkillTarget = (preferred: SkillTarget): SkillTarget => {
    const validTargets = visibleAgents.filter((a): a is SkillTarget => a === "claude_code" || a === "codex" || a === "pi");
    if (validTargets.includes(preferred)) return preferred;
    return validTargets[0] ?? "claude_code";
  };

  const [internalTarget, setInternalTarget] = useState<SkillTarget>(() => getValidSkillTarget(readSkillsTarget()));
  const target = targetProp ?? internalTarget;

  useEffect(() => {
    if (!targetProp) {
      const valid = getValidSkillTarget(internalTarget);
      if (valid !== internalTarget) {
        setInternalTarget(valid);
      }
    }
  }, [visibleAgents, internalTarget, targetProp]);
  const skillsQuery = useQuery(skillsOptions(target));
  const unmanagedQuery = useQuery(unmanagedSkillsOptions(target));
  const repositoriesQuery = useQuery(skillRepositoriesOptions);
  const skills = skillsQuery.data ?? [];
  const unmanagedSkills = unmanagedQuery.data ?? [];
  const repositories = repositoriesQuery.data ?? [];

  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [checkingSkill, setCheckingSkill] = useState<string | null>(null);
  const [zipOpen, setZipOpen] = useState(false);
  const [repositoryOpen, setRepositoryOpen] = useState(false);
  const [zipPath, setZipPath] = useState("");

  // Repository management modal state
  const [activeRepo, setActiveRepo] = useState<SkillRepositorySnapshot | null>(null);
  const [newRepoUrl, setNewRepoUrl] = useState("");
  const [searchText, setSearchText] = useState("");
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [addingRepo, setAddingRepo] = useState(false);
  const [refreshingRepoUrl, setRefreshingRepoUrl] = useState<string | null>(null);

  const [updateStatuses, setUpdateStatuses] = useState<Record<string, SkillUpdateStatus>>({});
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [discoveryBusy, setDiscoveryBusy] = useState<string | null>(null);

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(SKILLS_TARGET_KEY, target);
    }
    setUpdateStatuses({});
  }, [target]);

  // Keep activeRepo in sync with fresh query data if refreshed
  useEffect(() => {
    if (!activeRepo || !repositories.length) return;
    const updated = repositories.find((r) => r.repositoryUrl === activeRepo.repositoryUrl);
    if (updated && updated !== activeRepo) {
      setActiveRepo(updated);
    }
  }, [repositories, activeRepo]);

  const handleAddRepository = async (url: string) => {
    const trimmed = url.trim();
    if (!trimmed) return;
    setAddingRepo(true);
    try {
      await addSkillRepository(trimmed);
      await queryClient.invalidateQueries({ queryKey: ["skillRepositories"] });
      setNewRepoUrl("");
      void message.success(t("skills.repositoryAdded"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setAddingRepo(false);
    }
  };

  const handleRefreshRepository = async (url: string) => {
    setRefreshingRepoUrl(url);
    try {
      const snapshot = await refreshGithubRepositorySkills(url);
      await queryClient.invalidateQueries({ queryKey: ["skillRepositories"] });
      if (activeRepo && activeRepo.repositoryUrl === snapshot.repositoryUrl) {
        setActiveRepo(snapshot);
      }
      void message.success(t("skills.repositoryLastUpdated", { time: new Date().toLocaleTimeString() }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setRefreshingRepoUrl(null);
    }
  };

  const handleRemoveRepository = (url: string) => {
    Modal.confirm({
      title: t("skills.confirmRemoveRepositoryTitle"),
      content: t("skills.confirmRemoveRepositoryContent"),
      okText: t("skills.delete"),
      okType: "danger",
      cancelText: t("providers.cancel"),
      onOk: async () => {
        try {
          await removeSkillRepository(url);
          await queryClient.invalidateQueries({ queryKey: ["skillRepositories"] });
          if (activeRepo?.repositoryUrl === url) {
            setActiveRepo(null);
          }
          void message.success(t("skills.repositoryRemoved"));
        } catch (e) {
          void message.error(errMsg(e));
        }
      },
    });
  };

  const handleInstallFromActiveRepo = async () => {
    if (!activeRepo || !selectedPaths.length) return;
    setBusy(true);
    try {
      const installed = await installGithubRepositorySkills(activeRepo.repositoryUrl, selectedPaths, target);
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

  const handleShareSkill = async (skill: Skill) => {
    try {
      const url = skill.source?.sourceUrl || `https://github.com/anthropics/skills/tree/main/${skill.name}`;
      const link = await buildSkillDeeplink(skill.name, url);
      await navigator.clipboard.writeText(link);
      void message.success(t("deeplink.linkCopied"));
    } catch (e) {
      void message.error(errMsg(e));
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

  // Filter skills inside active repository
  const filteredRepoSkills = (activeRepo?.skills ?? []).filter((s) => {
    if (!searchText.trim()) return true;
    const q = searchText.toLowerCase();
    return s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q) || s.path.toLowerCase().includes(q);
  });

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <OnboardingTip tipKey="skills" message={t("skills.title")} description={t("skills.description")} />
      {!targetProp && (
        <WorkspaceTargetSegmented<SkillTarget>
          value={target}
          onChange={setInternalTarget}
          t={t}
          targets={["claude_code", "codex", "pi"]}
        />
      )}
      <Card
        size="small"
        className="page-surface"
        title={t("skills.discoveryTitle")}
        extra={
          <Button
            icon={<ReloadOutlined />}
            loading={unmanagedQuery.isFetching}
            disabled={busy}
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
                    disabled={busy || discoveryBusy !== null}
                    onClick={() => void registerUnmanaged(skill)}
                  >
                    {t("skills.discoveryRegister")}
                  </Button>
                  <Button
                    type="link"
                    disabled={busy || discoveryBusy !== null}
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
        extra={
          <Space>
            <Button icon={<GithubOutlined />} disabled={busy} onClick={() => setRepositoryOpen(true)}>
              {t("skills.repositoryTitle")}
            </Button>
            <Button loading={checkingUpdates} disabled={busy} onClick={() => void checkAllUpdates()}>
              {t("skills.checkAllUpdates")}
            </Button>
            <Button type="primary" disabled={!updateAvailableNames.length || busy} onClick={() => confirmUpdates(updateAvailableNames)}>
              {t("skills.updateSelected", { count: updateAvailableNames.length })}
            </Button>
            <Button icon={<ReloadOutlined />} loading={refreshing} onClick={() => void refreshSkills()}>
              {t("common.refresh")}
            </Button>
            <Button icon={<InboxOutlined />} onClick={() => setZipOpen(true)} disabled={busy}>
              {t("skills.installZip")}
            </Button>
          </Space>
        }
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
            { title: t("skills.enabled"), render: (_: unknown, skill: Skill) => <Switch checked={skill.enabled} disabled={busy} onChange={(checked) => void toggle(skill, checked)} /> },
            { title: t("skills.updateStatus"), render: (_: unknown, skill: Skill) => <SkillStatus status={updateStatuses[skill.name]} t={t} /> },
            { title: t("skills.actions"), render: (_: unknown, skill: Skill) => <Space size="small"><Button type="link" loading={checkingSkill === skill.name} disabled={busy || !skill.source} onClick={() => void checkUpdate(skill)}>{t("skills.checkUpdate")}</Button>{updateStatuses[skill.name]?.status === "update_available" && <Button type="link" disabled={busy} onClick={() => confirmUpdates([skill.name])}>{t("skills.updateSelected", { count: 1 })}</Button>}<Button type="link" disabled={busy} onClick={() => void handleShareSkill(skill)}>{t("deeplink.shareLink")}</Button><Button danger type="link" icon={<DeleteOutlined />} disabled={busy} onClick={() => void remove(skill)}>{t("skills.delete")}</Button></Space> },
          ]}
        />
      </Card>

      <Modal
        title={
          activeRepo ? (
            <Space>
              <Button
                type="text"
                icon={<ArrowLeftOutlined />}
                onClick={() => {
                  setActiveRepo(null);
                  setSelectedPaths([]);
                  setSearchText("");
                }}
              >
                {t("skills.backToRepositories")}
              </Button>
              <Text strong>{activeRepo.repositoryUrl}</Text>
            </Space>
          ) : (
            t("skills.repositoryTitle")
          )
        }
        open={repositoryOpen}
        width={880}
        footer={null}
        onCancel={() => {
          setRepositoryOpen(false);
          setActiveRepo(null);
          setSelectedPaths([]);
          setSearchText("");
        }}
      >
        {activeRepo ? (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Card size="small" className="page-surface">
              <Space direction="vertical" size="small" style={{ width: "100%" }}>
                <Space style={{ width: "100%", justifyContent: "space-between" }} wrap>
                  <Space align="center">
                    <Text type="secondary">{t("skills.targetAgent")}:</Text>
                    <Tag color="blue" style={{ margin: 0 }}>{getTargetLabel(target)}</Tag>
                  </Space>
                  <Space>
                    <Button
                      icon={<ReloadOutlined />}
                      loading={refreshingRepoUrl === activeRepo.repositoryUrl}
                      onClick={() => void handleRefreshRepository(activeRepo.repositoryUrl)}
                    >
                      {t("common.refresh")}
                    </Button>
                    <Button
                      type="primary"
                      icon={<DownloadOutlined />}
                      loading={busy}
                      disabled={!selectedPaths.length}
                      onClick={() => void handleInstallFromActiveRepo()}
                    >
                      {t("skills.installSelected")}
                      {selectedPaths.length > 0 ? ` (${selectedPaths.length})` : ""}
                    </Button>
                  </Space>
                </Space>
                <Input
                  prefix={<SearchOutlined />}
                  placeholder={t("skills.searchSkillsPlaceholder")}
                  value={searchText}
                  onChange={(e) => setSearchText(e.target.value)}
                  allowClear
                />
              </Space>
            </Card>

            <Table
              size="small"
              rowKey="path"
              dataSource={filteredRepoSkills}
              loading={refreshingRepoUrl === activeRepo.repositoryUrl}
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
        ) : (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Text type="secondary">{t("skills.repositoryHelp")}</Text>
            <Space style={{ width: "100%" }} wrap>
              <Input
                prefix={<GithubOutlined />}
                placeholder={t("skills.addRepositoryPlaceholder")}
                value={newRepoUrl}
                style={{ width: 420 }}
                onChange={(e) => setNewRepoUrl(e.target.value)}
                onPressEnter={() => void handleAddRepository(newRepoUrl)}
              />
              <Button
                type="primary"
                icon={<PlusOutlined />}
                loading={addingRepo}
                disabled={!newRepoUrl.trim()}
                onClick={() => void handleAddRepository(newRepoUrl)}
              >
                {t("skills.addRepository")}
              </Button>
              <Button
                disabled={addingRepo}
                onClick={() => void handleAddRepository(DEFAULT_SKILL_REPOSITORY)}
              >
                {t("skills.restoreDefaultRepository")}
              </Button>
            </Space>

            <Table
              size="small"
              rowKey="repositoryUrl"
              dataSource={repositories}
              loading={repositoriesQuery.isLoading}
              pagination={false}
              locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("skills.repositoryEmpty")} /> }}
              columns={[
                {
                  title: t("skills.repositoryUrl"),
                  dataIndex: "repositoryUrl",
                  render: (url: string) => (
                    <Space>
                      <GithubOutlined />
                      <Text copyable={{ text: url }} strong>{url}</Text>
                    </Space>
                  ),
                },
                {
                  title: t("skills.skillCount"),
                  dataIndex: "skills",
                  width: 120,
                  render: (skills: RepositorySkill[]) => (
                    <Tag color="blue">{t("skills.repositorySkillCount", { count: skills.length })}</Tag>
                  ),
                },
                {
                  title: t("skills.lastFetched"),
                  dataIndex: "fetchedAt",
                  width: 180,
                  render: (fetchedAt?: number | null) =>
                    fetchedAt ? <Text type="secondary">{new Date(fetchedAt).toLocaleString()}</Text> : <Text type="secondary">{t("skills.neverFetched")}</Text>,
                },
                {
                  title: t("skills.actions"),
                  width: 240,
                  render: (_: unknown, repo: SkillRepositorySnapshot) => (
                    <Space size="small">
                      <Button
                        type="link"
                        icon={<RightOutlined />}
                        onClick={() => {
                          setActiveRepo(repo);
                          setSelectedPaths([]);
                          setSearchText("");
                        }}
                      >
                        {t("skills.enterRepository")}
                      </Button>
                      <Button
                        type="link"
                        icon={<ReloadOutlined />}
                        loading={refreshingRepoUrl === repo.repositoryUrl}
                        onClick={() => void handleRefreshRepository(repo.repositoryUrl)}
                      >
                        {t("common.refresh")}
                      </Button>
                      <Button
                        danger
                        type="link"
                        icon={<DeleteOutlined />}
                        onClick={() => handleRemoveRepository(repo.repositoryUrl)}
                      >
                        {t("skills.delete")}
                      </Button>
                    </Space>
                  ),
                },
              ]}
            />
          </Space>
        )}
      </Modal>

      <Modal title={t("skills.installZip")} open={zipOpen} confirmLoading={busy} onOk={() => void installZip()} onCancel={() => setZipOpen(false)}>
        <Space direction="vertical" style={{ width: "100%" }}>
          <Text type="secondary">{t("skills.zipHelp")}</Text>
          <Input placeholder="C:\\Downloads\\my-skill.zip" value={zipPath} onChange={(e) => setZipPath(e.target.value)} onPressEnter={() => void installZip()} />
        </Space>
      </Modal>
    </Space>
  );
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
