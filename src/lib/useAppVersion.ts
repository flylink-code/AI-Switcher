import { getVersion } from "@tauri-apps/api/app";
import { useQuery } from "@tanstack/react-query";

export function useAppVersion(): string | null {
  const query = useQuery({
    queryKey: ["app-version"],
    queryFn: getVersion,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: 1,
  });
  return query.data ?? null;
}
