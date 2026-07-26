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

const pageTasks: StartupTask[] = [
  { id: "providersPage", run: () => import("@/pages/ProvidersPage") },
  { id: "proxyPage", run: () => import("@/pages/ProxyPage") },
  { id: "mcpPage", run: () => import("@/pages/McpPage") },
  { id: "promptsPage", run: () => import("@/pages/PromptsPage") },
  { id: "skillsPage", run: () => import("@/pages/SkillsPage") },
  { id: "usagePage", run: () => import("@/pages/UsagePage") },
  { id: "environmentPage", run: () => import("@/pages/EnvironmentPage") },
  { id: "localizationPage", run: () => import("@/pages/DesktopLocalizationPage") },
  { id: "aboutPage", run: () => import("@/pages/AboutPage") },
];

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
  { id: "promptsData", run: () => queryClient.fetchQuery(promptsOverviewOptions) },
  { id: "skillsData", run: () => queryClient.fetchQuery(skillsOptions) },
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
    run: () => continueAfterTimeout(queryClient.fetchQuery(localizationOptions), 1_500),
  },
  {
    id: "versionData",
    run: () => continueAfterTimeout(queryClient.fetchQuery(localClaudeVersionOptions), 1_500),
  },
];

const TOTAL_TASKS =
  criticalTasks.length + localDataTasks.length + pageTasks.length + slowTasks.length;

export async function runStartupWarmup(
  onProgress: (progress: StartupProgress) => void,
): Promise<StartupProgress> {
  const state: StartupProgress = {
    completed: 0,
    total: TOTAL_TASKS,
    current: "starting",
    failures: [],
  };
  const report = () => onProgress({ ...state, failures: [...state.failures] });
  report();

  await runTasks(criticalTasks, 2, state, report);
  await runTasks(localDataTasks, 3, state, report);
  await runTasks(pageTasks, 2, state, report);
  await runTasks(slowTasks, 1, state, report);
  state.current = "done";
  report();
  return state;
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

async function continueAfterTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T | null> {
  return Promise.race([
    promise,
    new Promise<null>((resolve) => window.setTimeout(() => resolve(null), timeoutMs)),
  ]);
}
