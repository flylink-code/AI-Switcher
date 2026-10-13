// 中文化资源版本比较的纯函数。注意：本文件不得有运行时 import，
// 以便 scripts/test-localization-version.mjs 直接转译执行。

export type VersionRelation = "newer" | "same" | "unknown";

/** 去掉首尾空白与 `v` 前缀；空值返回 null。 */
export function normalizeResourceVersion(value?: string | null): string | null {
  const trimmed = value?.trim().replace(/^v/i, "");
  return trimmed ? trimmed : null;
}

/**
 * 比较本机与上游资源版本。
 * - 没有上游版本：unknown（按钮保持可点）
 * - 没有本机版本：newer
 * - 上游更旧或相同：same
 */
export function compareResourceVersion(
  local?: string | null,
  remote?: string | null,
): VersionRelation {
  const from = normalizeResourceVersion(local);
  const to = normalizeResourceVersion(remote);
  if (!to) return "unknown";
  if (!from) return "newer";
  if (from === to) return "same";
  const fromParts = from.split(/[.-]/).map((part) => Number.parseInt(part, 10));
  const toParts = to.split(/[.-]/).map((part) => Number.parseInt(part, 10));
  if (fromParts.some(Number.isNaN) || toParts.some(Number.isNaN)) {
    return "unknown";
  }
  const length = Math.max(fromParts.length, toParts.length);
  for (let index = 0; index < length; index += 1) {
    const a = fromParts[index] ?? 0;
    const b = toParts[index] ?? 0;
    if (b > a) return "newer";
    if (b < a) return "same";
  }
  return "same";
}
