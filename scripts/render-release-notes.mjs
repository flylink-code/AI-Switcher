#!/usr/bin/env node
/**
 * Build bilingual GitHub Release notes from About-page changelog i18n.
 * Fails if zh-CN / en-US `about.changelog.{x_y_z}` is missing or empty.
 *
 * Usage:
 *   node scripts/render-release-notes.mjs [outfile.md]
 *   RELEASE_VERSION=1.3.26 node scripts/render-release-notes.mjs -
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
const key = changelogKey(version);
const enNotes = notesFor("src/i18n/locales/en-US.json", key);
const zhNotes = notesFor("src/i18n/locales/zh-CN.json", key);

const body = `## AI-Switcher v${version}

## What's new

${bullets(enNotes)}

## 更新内容

${bullets(zhNotes)}

### Windows

Prefer the NSIS \`.exe\` for per-user installs without admin. MSI still requires elevation.
In-app updates are signed and delivered from this release.
建议使用 NSIS \`.exe\`（当前用户安装，通常无需管理员）；MSI 仍会请求提升权限。
应用内更新会从本 Release 获取已签名更新。

### Linux (preview)

Prefer the \`.AppImage\` (\`chmod +x\`, no sudo). \`.deb\` installs/updates need administrator privileges.
Requires Ubuntu 22.04 / Debian 12 or newer (\`libwebkit2gtk-4.1\`). Ubuntu 18.04 / 20.04 cannot run Tauri 2.
Linux builds are best-effort: Claude Desktop localization / some Windows-only tools are unavailable.
建议优先使用 \`.AppImage\`（先 \`chmod +x\`，无需 sudo）；\`.deb\` 需要管理员权限。
需要 Ubuntu 22.04 / Debian 12 或更新（\`libwebkit2gtk-4.1\`）。18.04 / 20.04 无法运行 Tauri 2。
Linux 为尽力支持：Claude Desktop 中文化等 Windows 专用能力不可用。
`;

const out = process.argv[2] ?? "RELEASE_NOTES.md";
if (out === "-") {
  process.stdout.write(body);
} else {
  writeFileSync(out, body, "utf8");
  console.error(`Wrote ${out} for v${version} (${key}: ${zhNotes.length} zh / ${enNotes.length} en)`);
}
