import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

export const USAGE_LOG_RECORDED_EVENT = "usage-log-recorded";

/** 已有查询可能早于日志事件取到快照；先等待它，再补一次查询，不取消在途请求。 */
export async function refreshUsageQuery<T>(query: {
  isFetching: boolean;
  refetch: (options: { cancelRefetch: boolean }) => Promise<T>;
}): Promise<T> {
  const wasFetching = query.isFetching;
  const result = await query.refetch({ cancelRefetch: false });
  return wasFetching ? query.refetch({ cancelRefetch: false }) : result;
}

export interface UseUsageLogRefreshOptions {
  enabled?: boolean;
  /** 调用方应返回 refetch({ cancelRefetch: false }) 的 Promise。 */
  onRefresh: () => Promise<unknown> | void;
  pollIntervalMs?: number | null;
  throttleMs?: number;
  catchUpOnVisible?: boolean;
}

const subscribers = new Set<() => void>();
let listenerGeneration = 0;
let unlisten: (() => void) | undefined;

function subscribe(callback: () => void): () => void {
  subscribers.add(callback);
  if (subscribers.size === 1) {
    const generation = ++listenerGeneration;
    void listen(USAGE_LOG_RECORDED_EVENT, () => {
      if (generation === listenerGeneration) {
        for (const subscriber of subscribers) subscriber();
      }
    }).then((dispose) => {
      if (generation !== listenerGeneration || subscribers.size === 0) dispose();
      else unlisten = dispose;
    }).catch(() => {
      // 事件通道不可用时保留轮询兜底。
    });
  }
  return () => {
    subscribers.delete(callback);
    if (subscribers.size === 0) {
      listenerGeneration++;
      unlisten?.();
      unlisten = undefined;
    }
  };
}

/** 状态属于本次订阅，旧 effect 的异步收尾不能改写新订阅的定时器。 */
export function useUsageLogRefresh({
  enabled = true,
  onRefresh,
  pollIntervalMs = null,
  throttleMs = 1_000,
  catchUpOnVisible = true,
}: UseUsageLogRefreshOptions): void {
  const onRefreshRef = useRef(onRefresh);
  useEffect(() => { onRefreshRef.current = onRefresh; }, [onRefresh]);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let inFlight = false;
    let dirty = false;
    let lastRefreshAt = -Infinity;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const visible = () => document.visibilityState === "visible";

    const schedule = () => {
      if (disposed || !dirty || inFlight || timer !== undefined || !visible()) return;
      timer = setTimeout(() => {
        timer = undefined;
        void execute();
      }, Math.max(0, throttleMs - (Date.now() - lastRefreshAt)));
    };
    const execute = async () => {
      if (disposed || !visible()) return;
      inFlight = true;
      dirty = false;
      lastRefreshAt = Date.now();
      try {
        await onRefreshRef.current();
      } catch {
        // 查询错误由调用方展示，后续事件和轮询仍可重试。
      } finally {
        inFlight = false;
        schedule();
      }
    };
    const trigger = () => {
      dirty = true;
      schedule();
    };
    const onVisibility = () => {
      if (catchUpOnVisible && visible()) trigger();
    };
    const unsubscribe = subscribe(trigger);
    document.addEventListener("visibilitychange", onVisibility);
    const poll = pollIntervalMs && pollIntervalMs > 0
      ? setInterval(trigger, pollIntervalMs)
      : undefined;
    return () => {
      disposed = true;
      clearTimeout(timer);
      clearInterval(poll);
      document.removeEventListener("visibilitychange", onVisibility);
      unsubscribe();
    };
  }, [enabled, pollIntervalMs, throttleMs, catchUpOnVisible]);
}
