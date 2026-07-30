/** Default remote library root for an SSH/WSL Linux user. */
export function defaultRemoteRootForUser(user: string): string {
  const safe = user.trim() || "user";
  return `/home/${safe}/.ai-switcher`;
}

/** True when remoteRoot still looks like the auto-managed home template. */
export function isManagedRemoteRoot(remoteRoot: string, previousUser?: string): boolean {
  const value = remoteRoot.trim();
  if (value === "/home/user/.ai-switcher") return true;
  if (previousUser && value === defaultRemoteRootForUser(previousUser)) return true;
  return /^\/home\/[^/]+\/\.ai-switcher$/.test(value);
}

export function nextRemoteRootForUser(
  remoteRoot: string,
  previousUser: string,
  nextUser: string,
): string {
  if (!nextUser.trim()) return remoteRoot;
  if (!isManagedRemoteRoot(remoteRoot, previousUser)) return remoteRoot;
  return defaultRemoteRootForUser(nextUser);
}

/** Split `user@host` (or host-only) into parts. */
export function splitSshEndpoint(value: string): { user: string; host: string } {
  const trimmed = value.trim();
  const at = trimmed.lastIndexOf("@");
  if (at <= 0) return { user: "", host: trimmed };
  return { user: trimmed.slice(0, at), host: trimmed.slice(at + 1) };
}

export function joinSshEndpoint(user: string, host: string): string {
  const u = user.trim();
  const h = host.trim();
  if (!h) return "";
  return u ? `${u}@${h}` : h;
}
