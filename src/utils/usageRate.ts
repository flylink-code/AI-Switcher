export interface TokenRateInput {
  outputTokens?: number | null;
  durationMs?: number | null;
  usageAvailable?: boolean | null;
  dataSource?: string | null;
}

export function calculateTokenRate(
  inputOrOutputTokens: TokenRateInput | number | null | undefined,
  durationMs?: number | null,
): number | null {
  const input =
    typeof inputOrOutputTokens === "object" && inputOrOutputTokens !== null
      ? inputOrOutputTokens
      : { outputTokens: inputOrOutputTokens, durationMs };

  if (input.usageAvailable === false || input.dataSource?.endsWith("_session")) {
    return null;
  }
  if (
    input.outputTokens == null ||
    input.durationMs == null ||
    !Number.isFinite(input.outputTokens) ||
    !Number.isFinite(input.durationMs) ||
    input.outputTokens <= 0 ||
    input.durationMs <= 0
  ) {
    return null;
  }

  const rate = (input.outputTokens * 1000) / input.durationMs;
  return Number.isFinite(rate) && rate > 0 ? rate : null;
}

export function formatTokenRate(
  inputOrOutputTokens: TokenRateInput | number | null | undefined,
  durationMs?: number | null,
): string {
  const rate = calculateTokenRate(inputOrOutputTokens, durationMs);
  return rate == null ? "—" : `${rate.toFixed(1)} Token/s`;
}
