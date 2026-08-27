#!/usr/bin/env node
/**
 * Build GitHub Release notes from About-page changelog i18n.
 * Fails if zh-CN / en-US `about.changelog.{x_y_z}` is missing or empty.
 *
 * Usage:
 *   node scripts/render-release-notes.mjs [outfile.md]
 *   RELEASE_VERSION=1.4.0 node scripts/render-release-notes.mjs -
 *   RELEASE_ASSETS=$'AI-Switcher_1.4.0_x64-setup.exe\n...' node scripts/render-release-notes.mjs
 *
 * RELEASE_ASSETS is a newline-separated list of uploaded filenames. When empty,
 * the download section says installers will appear after the build finishes.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function readJson(relativePath) {
  return JSON.parse(readFileSync(join(root, relativePath), "utf8"));
}

function changelogKey(version) {
  return version.replaceAll(".", "_");
}

function notesFor(localeFile, key) {
  const data = readJson(localeFile);
  const notes = data?.about?.changelog?.[key];
  if (!Array.isArray(notes) || notes.length === 0) {
    throw new Error(`Missing or empty about.changelog.${key} in ${localeFile}`);
  }
  const cleaned = notes.map((line) => String(line).trim()).filter(Boolean);
  if (cleaned.length === 0) {
    throw new Error(`about.changelog.${key} in ${localeFile} has no text`);
  }
  return cleaned;
}

function bullets(lines) {
  return lines.map((line) => `- ${line}`).join("\n");
}

function shanghaiStamp() {
  return new Intl.DateTimeFormat("sv-SE", {
    timeZone: "Asia/Shanghai",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date());
}

function parseAssetNames(raw) {
  return String(raw ?? "")
    .split(/\r?\n/)
    .map((name) => name.trim())
    .filter(Boolean);
}

function isInstallerAsset(name) {
  const lower = name.toLowerCase();
  if (lower.endsWith(".sig")) return false;
  if (lower === "latest.json" || lower === "latest-mirror.json") return false;
  return (
    lower.endsWith("-setup.exe") ||
    lower.endsWith(".exe") ||
    lower.endsWith(".msi") ||
    lower.endsWith(".appimage") ||
    lower.endsWith(".deb")
  );
}

function classifyAsset(name) {
  const lower = name.toLowerCase();
  if (lower.endsWith("-setup.exe") || (lower.endsWith(".exe") && !lower.endsWith(".sig"))) {
    return "windows-nsis";
  }
  if (lower.endsWith(".msi")) return "windows-msi";
  if (lower.endsWith(".appimage")) return "linux-appimage";
  if (lower.endsWith(".deb")) return "linux-deb";
  return "other";
}

function downloadUrl(repo, tag, filename) {
  return `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(filename)}`;
}

function link(label, repo, tag, filename) {
  return `- [${label}](${downloadUrl(repo, tag, filename)})`;
}

function downloadSection(repo, tag, assets) {
  const installers = assets.filter(isInstallerAsset);
  if (installers.length === 0) {
    return `## 下载地址

安装包将在 Windows / Linux 构建完成后出现在本页 Assets。构建结束后会自动补上直链。
Installers appear in Assets after the Windows / Linux jobs finish; this section is filled in then.`;
  }

  const nsis = installers.filter((name) => classifyAsset(name) === "windows-nsis");
  const msi = installers.filter((name) => classifyAsset(name) === "windows-msi");
  const appimage = installers.filter((name) => classifyAsset(name) === "linux-appimage");
  const deb = installers.filter((name) => classifyAsset(name) === "linux-deb");

  const windowsLines = [
    ...nsis.map((name) => link(`${name}（推荐，当前用户安装）`, repo, tag, name)),
    ...msi.map((name) => link(`${name}（需管理员）`, repo, tag, name)),
  ];
  const linuxLines = [
    ...appimage.map((name) => link(`${name}（推荐，chmod +x 后运行）`, repo, tag, name)),
    ...deb.map((name) => link(`${name}（需管理员）`, repo, tag, name)),
  ];

  const windowsBlock =
    windowsLines.length > 0 ? windowsLines.join("\n") : "- （构建完成后列出 Windows 安装包）";
  const linuxBlock =
    linuxLines.length > 0 ? linuxLines.join("\n") : "- （构建完成后列出 Linux 安装包）";

  return `## 下载地址

### Windows
${windowsBlock}

### Linux（预览）
${linuxBlock}`;
}

const pkg = readJson("package.json");
const pkgVersion = String(pkg.version ?? "").trim();
const envVersion = String(process.env.RELEASE_VERSION ?? "")
  .trim()
  .replace(/^v/i, "");
const envIsSemver = /^\d+\.\d+\.\d+$/.test(envVersion);

if (!pkgVersion) {
  throw new Error("package.json version is empty");
}
if (envIsSemver && envVersion !== pkgVersion) {
  throw new Error(
    `RELEASE_VERSION ${envVersion} does not match package.json ${pkgVersion}`,
  );
}

const version = pkgVersion;
const tag = `v${version}`;
const repo = String(process.env.GITHUB_REPOSITORY ?? "flylink-code/AI-Switcher").trim();
const key = changelogKey(version);
const enNotes = notesFor("src/i18n/locales/en-US.json", key);
const zhNotes = notesFor("src/i18n/locales/zh-CN.json", key);
const assets = parseAssetNames(process.env.RELEASE_ASSETS);

const body = `## AI-Switcher v${version}

### 更新内容

${bullets(zhNotes)}

### What's new

${bullets(enNotes)}

${downloadSection(repo, tag, assets)}

### FAQ

- **Windows**：优先 NSIS \`.exe\`（当前用户安装，通常无需管理员）。MSI 仍会请求提升权限。
- **Linux**：优先 \`.AppImage\`（先 \`chmod +x\`）。需要 Ubuntu 22.04 / Debian 12 或更新（\`libwebkit2gtk-4.1\`）。18.04 / 20.04 无法运行 Tauri 2。
- **应用内更新**：从本 Release 下载已签名安装包。
- Prefer the NSIS \`.exe\` for per-user installs. MSI still requires elevation. Linux AppImage needs Ubuntu 22.04 / Debian 12+.

Created at ${shanghaiStamp()} (Asia/Shanghai).
`;

const out = process.argv[2] ?? "RELEASE_NOTES.md";
if (out === "-") {
  process.stdout.write(body);
} else {
  writeFileSync(out, body, "utf8");
  console.error(
    `Wrote ${out} for v${version} (${key}: ${zhNotes.length} zh / ${enNotes.length} en, assets=${assets.length})`,
  );
}
