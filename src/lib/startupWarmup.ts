import { queryClient } from "@/lib/queryClient";
import {
  autostartOptions,
  environmentOptions,
  localClaudeVersionOptions,
  localizationOptions,
  mcpServersOptions,
  promptsOverviewOptions,
  providerListOptions,
  proxyStatusOptions,
  skillsOptions,
  usageOverviewOptions,
} from "@/lib/appQueries";
import { PAGE_KEYS, preloadPage } from "@/lib/pageRegistry";
import { reportFrontendPerformance } from "@/services/api";

export interface StartupProgress {
  completed: number;
  total: number;
  current: string;
  failures: string[];
}

type StartupTask = {
  id: string;
  run: () => Promise<unknown>;
};

const criticalTasks: StartupTask[] = [
  {
    id: "codeProviders",
    run: () => queryClient.fetchQuery(providerListOptions("claude_code")),
  },
  {
    id: "desktopProviders",
    run: () => queryClient.fetchQuery(providerListOptions("claude_desktop")),
  },
  {
    id: "codeProxy",
    run: () => queryClient.fetchQuery(proxyStatusOptions("claude_code")),
  },
  {
    id: "desktopProxy",
    run: () => queryClient.fetchQuery(proxyStatusOptions("claude_desktop")),
  },
];

const localDataTasks: StartupTask[] = [
  { id: "mcpData", run: () => queryClient.fetchQuery(mcpServersOptions) },
  { id: "promptsData", run: () => queryClient.fetchQuery(promptsOverviewOptions("claude_code")) },
  { id: "skillsData", run: () => queryClient.fetchQuery(skillsOptions("claude_code")) },
  {
    id: "usageData",
    run: () => queryClient.fetchQuery(usageOverviewOptions(30, 0, "all")),
  },
  { id: "environmentData", run: () => queryClient.fetchQuery(environmentOptions) },
  { id: "autostartData", run: () => queryClient.fetchQuery(autostartOptions) },
];

const slowTasks: StartupTask[] = [
  {
    id: "localizationData",
    run: () => queryClient.fetchQuery(localizationOptions),
  },
  {
    id: "versionData",
    run: () => queryClient.fetchQuery(localClaudeVersionOptions),
  },
];

export async function runStartupWarmup(
  language: string,
  onProgress: (progress: StartupProgress) => void,
): Promise<StartupProgress> {
  const pageTasks = PAGE_KEYS
    .filter((key) => key !== "localization" || language === "zh-CN")
    .map((key) => ({ id: `${key}Page`, run: () => preloadPage(key) }));
  const enabledSlowTasks = slowTasks.filter(
    (task) => task.id !== "localizationData" || language === "zh-CN",
  );
  const state: StartupProgress = {
    completed: 0,
    total: pageTasks.length + criticalTasks.length,
    current: "starting",
    failures: [],
  };
  const report = () => onProgress({ ...state, failures: [...state.failures] });
  report();

  const pageModulesStartedAt = performance.now();
  await runTasks(pageTasks, 2, state, report);
  void reportFrontendPerformance(
    "startup_phase",
    "page_modules",
    performance.now() - pageModulesStartedAt,
  ).catch(() => undefined);

  const criticalDataStartedAt = performance.now();
  await runTasks(criticalTasks, 2, state, report);
  void reportFrontendPerformance(
    "startup_phase",
    "critical_data",
    performance.now() - criticalDataStartedAt,
  ).catch(() => undefined);
  state.current = "done";
  report();
  void runBackgroundWarmup(enabledSlowTasks);
  return state;
}

async function runBackgroundWarmup(enabledSlowTasks: StartupTask[]): Promise<void> {
  const backgroundState: StartupProgress = {
    completed: 0,
    total: localDataTasks.length + enabledSlowTasks.length,
    current: "background",
    failures: [],
  };
  const silentReport = () => undefined;
  await runTasks(localDataTasks, 3, backgroundState, silentReport);
  await runTasks(enabledSlowTasks, 2, backgroundState, silentReport);
}

async function runTasks(
  tasks: StartupTask[],
  concurrency: number,
  state: StartupProgress,
  report: () => void,
): Promise<void> {
  let next = 0;
  const worker = async () => {
    while (next < tasks.length) {
      const task = tasks[next++];
      state.current = task.id;
      report();
      try {
        await task.run();
      } catch {
        state.failures.push(task.id);
      } finally {
        state.completed += 1;
        report();
      }
    }
  };
  await Promise.all(
    Array.from({ length: Math.min(concurrency, tasks.length) }, () => worker()),
  );
}
