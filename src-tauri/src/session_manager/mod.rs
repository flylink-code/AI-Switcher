//! Local session discovery and archive operations for Claude Code and Codex.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::{ZipArchive, ZipWriter};
use zip::write::SimpleFileOptions;

use crate::config;
use crate::error::{AppError, AppResult};

const SEARCH_RESULT_LIMIT: usize = 200;
const SUMMARY_LIMIT: usize = 160;
const SESSION_ARCHIVE_VERSION: u8 = 1;
const SESSION_ARCHIVE_MANIFEST: &str = "manifest.json";
const SESSION_ARCHIVE_CONTENT: &str = "session.jsonl";
const SESSION_BATCH_ARCHIVE_VERSION: u8 = 1;
const SESSION_BATCH_ARCHIVE_MANIFEST: &str = "batch-manifest.json";
const SESSION_BATCH_ARCHIVE_PREFIX: &str = "sessions";
/// Soft cap so a huge session tree cannot freeze the UI indefinitely.
const MAX_SESSION_FILES: usize = 2_000;
/// Bound recursive walks under cloud-synced / AV-watched trees.
const MAX_WALK_DEPTH: u32 = 6;
const WALK_DEADLINE: Duration = Duration::from_secs(5);
/// Content search may open files; keep it tiny to avoid system-wide I/O stalls.
const MAX_CONTENT_SEARCH_OPENS: usize = 40;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionProvider {
    ClaudeCode,
    Codex,
    /// Keep wire format `opencode` (not `open_code`) to match ProviderTarget / frontend.
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "pi")]
    Pi,
    #[serde(rename = "dsh")]
    Dsh,
    #[serde(rename = "cline")]
    Cline,
}

impl Default for SessionProvider {
    fn default() -> Self { Self::ClaudeCode }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProviderStatus {
    pub provider: SessionProvider,
    pub status: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider: SessionProvider,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    pub source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
    /// Codex thread pin from `state_5.sqlite` (`threads.is_pinned`).
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionScanResult {
    pub sessions: Vec<SessionMeta>,
    pub providers: Vec<SessionProviderStatus>,
    pub total: usize,
    pub offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionArchiveManifest {
    pub version: u8,
    #[serde(default)]
    pub provider: SessionProvider,
    pub session_id: String,
    pub relative_path: String,
    pub created_at: i64,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionArchiveInfo {
    pub archive_path: String,
    pub session_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBatchBackupInfo {
    pub archives: Vec<SessionArchiveInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBatchExportInfo {
    pub archive_path: String,
    pub session_count: usize,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBackupArchiveInfo {
    pub archive_path: String,
    pub filename: String,
    pub provider: SessionProvider,
    pub session_count: usize,
    pub created_at: i64,
    pub file_size: u64,
    pub is_batch: bool,
    #[serde(default)]
    pub is_auto: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBatchRestoreResult {
    pub restored_count: usize,
    pub skipped_count: usize,
    pub total_count: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBatchArchiveManifest {
    version: u8,
    created_at: i64,
    sessions: Vec<SessionArchiveManifest>,
}

/// 扫描中间项：Claude/Codex 走惰性文件路径（列表页绝不开文件），
/// OpenCode 会话来自 SQLite/JSON 存储，元数据在扫描时已完整物化。
enum ScanItem {
    File(i64, PathBuf, SessionProvider),
    Materialized(SessionMeta),
}

impl ScanItem {
    fn sort_ts(&self) -> i64 {
        match self {
            ScanItem::File(mtime, _, _) => *mtime,
            ScanItem::Materialized(meta) => {
                meta.last_active_at.or(meta.created_at).unwrap_or(0)
            }
        }
    }

    fn tie_key(&self) -> String {
        match self {
            ScanItem::File(_, path, _) => path.to_string_lossy().into_owned(),
            ScanItem::Materialized(meta) => meta.session_id.clone(),
        }
    }
}

pub fn scan_sessions(
    provider: Option<SessionProvider>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> AppResult<SessionScanResult> {
    let mut indexed: Vec<ScanItem> = Vec::new();
    let mut providers = Vec::new();
    let mut truncated = false;
    let mut timed_out = false;

    if provider.is_none() || provider == Some(SessionProvider::ClaudeCode) {
        let (paths, status, was_truncated, walk_timed_out) = collect_claude_code_session_paths()?;
        truncated |= was_truncated;
        timed_out |= walk_timed_out;
        for (path, mtime) in paths {
            indexed.push(ScanItem::File(mtime, path, SessionProvider::ClaudeCode));
        }
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::Codex) {
        let (paths, status, was_truncated, walk_timed_out) = collect_codex_session_paths()?;
        truncated |= was_truncated;
        timed_out |= walk_timed_out;
        for (path, mtime) in paths {
            indexed.push(ScanItem::File(mtime, path, SessionProvider::Codex));
        }
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::OpenCode) {
        let (metas, status) = scan_opencode_sessions();
        for meta in metas {
            indexed.push(ScanItem::Materialized(meta));
        }
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::Pi) {
        let (metas, status) = scan_pi_sessions();
        for meta in metas {
            indexed.push(ScanItem::Materialized(meta));
        }
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::Dsh) {
        let (metas, status) = scan_dsh_sessions();
        for meta in metas {
            indexed.push(ScanItem::Materialized(meta));
        }
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::Cline) {
        let (metas, status) = scan_cline_sessions();
        for meta in metas {
            indexed.push(ScanItem::Materialized(meta));
        }
        providers.push(status);
    }

    let codex_index = if indexed
        .iter()
        .any(|item| matches!(item, ScanItem::File(_, _, SessionProvider::Codex)))
    {
        load_codex_thread_index()
    } else {
        CodexThreadIndex::default()
    };

    indexed.sort_by(|left, right| {
        let pinned = |item: &ScanItem| match item {
            ScanItem::File(_, path, SessionProvider::Codex) => {
                codex_index.lookup(path).is_some_and(|meta| meta.pinned)
            }
            _ => false,
        };
        pinned(right)
            .cmp(&pinned(left))
            .then_with(|| right.sort_ts().cmp(&left.sort_ts()))
            .then_with(|| left.tie_key().cmp(&right.tie_key()))
    });

    let total = indexed.len();
    let offset = offset.unwrap_or(0).min(total);
    let limit = limit.filter(|value| *value > 0);
    let page: Vec<ScanItem> = match limit {
        Some(limit) => indexed.into_iter().skip(offset).take(limit).collect(),
        None if offset > 0 => indexed.into_iter().skip(offset).collect(),
        None => indexed,
    };

    // List view must NEVER open session files. Opening cloud placeholders /
    // antivirus-locked jsonl on Windows can stall the kernel and freeze the OS.
    // Codex names / pins come from SQLite thread index instead.
    let sessions = page
        .into_iter()
        .filter_map(|item| match item {
            ScanItem::File(mtime, path, session_provider) => {
                let mut session = session_meta_from_path(session_provider, &path, mtime)?;
                if session_provider == SessionProvider::Codex {
                    apply_codex_thread_meta(&mut session, &path, &codex_index);
                }
                Some(session)
            }
            ScanItem::Materialized(meta) => Some(meta),
        })
        .collect();

    if truncated || timed_out {
        for status in &mut providers {
            let mut notes = Vec::new();
            if truncated {
                notes.push(format!("已限制最多扫描 {MAX_SESSION_FILES} 个会话文件"));
            }
            if timed_out {
                notes.push(format!(
                    "目录扫描超过 {} 秒已提前结束（可能被云同步或杀毒卡住）",
                    WALK_DEADLINE.as_secs()
                ));
            }
            if !notes.is_empty() {
                status.detail = format!("{}；{}", status.detail, notes.join("；"));
                if status.status == "available" {
                    status.status = "degraded".to_string();
                }
            }
        }
    }

    Ok(SessionScanResult {
        sessions,
        providers,
        total,
        offset,
        limit,
    })
}

pub fn search_session_contents(
    query: &str,
    provider: Option<SessionProvider>,
    limit: usize,
) -> AppResult<SessionScanResult> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err(AppError::Config("搜索内容不能为空".to_string()));
    }

    let mut result = scan_sessions(provider, None, None)?;
    let limit = clamp_search_limit(limit);
    let mut matched = Vec::new();
    let mut opens = 0usize;
    for session in result.sessions.drain(..) {
        if session_metadata_contains(&session, &query) {
            matched.push(session);
        } else if opens < MAX_CONTENT_SEARCH_OPENS {
            opens += 1;
            if file_contains(session.provider, &session.source_path, &query).unwrap_or(false) {
                matched.push(session);
            }
        }
        if matched.len() >= limit {
            break;
        }
    }
    result.sessions = matched;
    result.total = result.sessions.len();
    result.offset = 0;
    result.limit = Some(limit);
    Ok(result)
}

fn load_pi_messages(source_path: &str) -> AppResult<Vec<SessionMessage>> {
    let path = Path::new(source_path);
    if !path.exists() {
        return Err(AppError::Config(format!("Pi 会话文件不存在: {source_path}")));
    }
    let content = fs::read_to_string(path)?;
    let mut messages = Vec::new();

    if path.extension().is_some_and(|e| e == "json") {
        if let Ok(val) = serde_json::from_str::<Value>(&content) {
            if let Some(arr) = val.get("messages").and_then(Value::as_array) {
                for item in arr {
                    let role = item.get("role").and_then(Value::as_str).unwrap_or("user").to_string();
                    let text = item.get("content").and_then(Value::as_str)
                        .or_else(|| item.get("text").and_then(Value::as_str))
                        .unwrap_or("").to_string();
                    if !text.is_empty() {
                        messages.push(SessionMessage { role, content: text, timestamp: None });
                    }
                }
            }
        }
    } else {
        for line in content.lines() {
            if line.trim().is_empty() { continue; }
            if let Ok(val) = serde_json::from_str::<Value>(line) {
                let role = val.get("role").and_then(Value::as_str).unwrap_or("user").to_string();
                let text = val.get("content").and_then(Value::as_str)
                    .or_else(|| val.get("text").and_then(Value::as_str))
                    .unwrap_or("").to_string();
                if !text.is_empty() {
                    messages.push(SessionMessage { role, content: text, timestamp: None });
                }
            }
        }
    }

    if messages.is_empty() {
        messages.push(SessionMessage {
            role: "system".to_string(),
            content,
            timestamp: None,
        });
    }

    Ok(messages)
}

fn dsh_text(content: Option<&Value>) -> String {
    content
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks.iter().filter_map(|block| {
                block.get("text").and_then(Value::as_str)
                    .or_else(|| block.get("content").and_then(Value::as_str))
            }).collect::<Vec<_>>().join("\n")
        })
        .unwrap_or_default()
}

fn load_dsh_messages(source_path: &str) -> AppResult<Vec<SessionMessage>> {
    let root = crate::config::get_dsh_config_dir().join("sessions");
    let source = validate_session_path_in_root(&root, Path::new(source_path))?;
    let events = crate::usage::session_usage_dsh::read_dsh_events(&source)
        .map_err(AppError::Config)?;
    let mut messages = Vec::new();
    for event in events {
        let timestamp = event.get("time").and_then(Value::as_i64);
        match event.get("type").and_then(Value::as_str) {
            Some("user/message") => {
                let data = event.get("data");
                let content = dsh_text(data.and_then(|value| value.get("content")));
                if !content.is_empty() {
                    messages.push(SessionMessage { role: "user".to_string(), content, timestamp });
                }
            }
            Some("assistant/message") => {
                let message = event.pointer("/data/message");
                let content = dsh_text(message.and_then(|value| value.get("content")));
                if !content.is_empty() {
                    messages.push(SessionMessage { role: "assistant".to_string(), content, timestamp });
                }
            }
            _ => {}
        }
    }
    Ok(messages)
}

pub fn load_session_messages(
    provider: SessionProvider,
    source_path: &str,
) -> AppResult<Vec<SessionMessage>> {
    match provider {
        SessionProvider::ClaudeCode => {
            let root = claude_code_session_root();
            let source = validate_session_path_in_root(&root, Path::new(source_path))?;
            load_claude_code_messages(&source)
        }
        SessionProvider::Codex => {
            let root = codex_session_root();
            let source = validate_session_path_in_root(&root, Path::new(source_path))?;
            load_codex_messages(&source)
        }
        SessionProvider::OpenCode => load_opencode_messages(source_path),
        SessionProvider::Pi => load_pi_messages(source_path),
        SessionProvider::Dsh => load_dsh_messages(source_path),
        SessionProvider::Cline => load_cline_messages(source_path),
    }
}

/// Provider-aware session archive operations. The legacy Claude Code helpers
/// below intentionally remain as IPC-compatible wrappers.
pub fn export_session(provider: SessionProvider, source_path: &str, destination_dir: Option<&str>) -> AppResult<SessionArchiveInfo> {
    let (source, relative) = validated_session(provider, source_path)?;
    let content = fs::read(&source)?;
    let meta = parse_session(provider, &source)?.ok_or_else(|| AppError::Config("无法读取会话元数据".to_string()))?;
    let manifest = session_manifest(provider, &meta, relative, &content);
    let dir = resolve_export_dir(destination_dir, "session-archives")?;
    let archive_path = dir.join(format!("{}-{}.zip", safe_session_name(&manifest.session_id), manifest.created_at));
    write_session_archive(&archive_path, &manifest, &content)?;
    Ok(SessionArchiveInfo { archive_path: archive_path.to_string_lossy().into_owned(), session_id: manifest.session_id, created_at: manifest.created_at })
}

pub fn export_session_markdown(
    provider: SessionProvider,
    source_path: &str,
    destination_dir: Option<&str>,
) -> AppResult<String> {
    let messages = load_session_messages(provider, source_path)?;
    let (source, _) = validated_session(provider, source_path)?;
    let meta = parse_session(provider, &source)?;

    let title = meta
        .as_ref()
        .and_then(|m| m.title.as_deref())
        .or_else(|| meta.as_ref().map(|m| m.session_id.as_str()))
        .unwrap_or("Untitled Session");
    let session_id = meta.as_ref().map(|m| m.session_id.as_str()).unwrap_or("unknown");
    let project_dir = meta.as_ref().and_then(|m| m.project_dir.as_deref()).unwrap_or("N/A");

    let mut md = String::new();
    md.push_str(&format!("# Session: {title}\n\n"));
    md.push_str(&format!("- **Provider**: {:?}\n", provider));
    md.push_str(&format!("- **Session ID**: `{session_id}`\n"));
    md.push_str(&format!("- **Project**: `{project_dir}`\n"));
    if let Some(created_at) = meta.as_ref().and_then(|m| m.created_at) {
        let dt = chrono::DateTime::from_timestamp_millis(created_at)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_default();
        if !dt.is_empty() {
            md.push_str(&format!("- **Created**: {dt}\n"));
        }
    }
    md.push_str("\n---\n\n");

    for msg in messages {
        let role_label = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "system" => "System",
            other => other,
        };
        md.push_str(&format!("### {role_label}\n\n{}\n\n", msg.content.trim()));
    }

    let dir = resolve_export_dir(destination_dir, "session-markdown")?;
    let filename = format!("{}-{}.md", safe_session_name(session_id), chrono::Utc::now().timestamp());
    let export_path = dir.join(filename);
    fs::write(&export_path, md)?;

    Ok(export_path.to_string_lossy().into_owned())
}

pub fn backup_sessions(provider: SessionProvider, source_paths: &[String]) -> AppResult<SessionBatchBackupInfo> {
    let source_paths = unique_session_paths(source_paths)?;
    let mut archives = Vec::with_capacity(source_paths.len());
    for source_path in source_paths { archives.push(export_session(provider, &source_path, None)?); }
    Ok(SessionBatchBackupInfo { archives })
}

pub fn export_sessions(provider: SessionProvider, source_paths: &[String], destination_dir: Option<&str>) -> AppResult<SessionBatchExportInfo> {
    let source_paths = unique_session_paths(source_paths)?;
    let mut sessions = Vec::with_capacity(source_paths.len());
    for source_path in source_paths {
        let (source, relative) = validated_session(provider, &source_path)?;
        let content = fs::read(&source)?;
        let meta = parse_session(provider, &source)?.ok_or_else(|| AppError::Config("无法读取会话元数据".to_string()))?;
        sessions.push((session_manifest(provider, &meta, relative, &content), content));
    }
    let created_at = chrono::Utc::now().timestamp_millis();
    let dir = resolve_export_dir(destination_dir, "session-exports")?;
    let name = match provider { SessionProvider::Codex => "codex", _ => "claude-code" };
    let archive_path = dir.join(format!("{name}-sessions-{created_at}.zip"));
    write_batch_session_archive(&archive_path, created_at, &sessions)?;
    Ok(SessionBatchExportInfo { archive_path: archive_path.to_string_lossy().into_owned(), session_count: sessions.len(), created_at })
}

pub fn import_session(provider: SessionProvider, archive_path: &str) -> AppResult<SessionMeta> {
    if is_batch_session_archive(Path::new(archive_path))? {
        return import_sessions(provider, archive_path)?.into_iter().next().ok_or_else(|| AppError::Config("会话批量归档为空".to_string()));
    }
    let (manifest, content) = read_session_archive(Path::new(archive_path))?;
    validate_manifest_provider(provider, &manifest)?;
    let target = import_target(provider, &manifest.relative_path)?;
    if target.exists() {
        if hex::encode(Sha256::digest(fs::read(&target)?)) != manifest.content_sha256 { return Err(AppError::Config("目标位置已有不同的会话，已拒绝覆盖".to_string())); }
    } else {
        if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
        crate::config::atomic_write(&target, &content)?;
    }
    parse_session(provider, &target)?.ok_or_else(|| AppError::Config("导入的会话内容无效".to_string()))
}

pub fn import_sessions(provider: SessionProvider, archive_path: &str) -> AppResult<Vec<SessionMeta>> {
    let (batch, contents) = read_batch_session_archive(Path::new(archive_path))?;
    let mut targets = Vec::with_capacity(batch.sessions.len());
    let mut target_paths = std::collections::HashSet::new();
    for (manifest, content) in batch.sessions.iter().zip(contents.iter()) {
        validate_manifest_provider(provider, manifest)?;
        let target = import_target(provider, &manifest.relative_path)?;
        if !target_paths.insert(target.clone()) { return Err(AppError::Config("会话批量归档包含重复的目标路径".to_string())); }
        if target.exists() && hex::encode(Sha256::digest(fs::read(&target)?)) != manifest.content_sha256 { return Err(AppError::Config("目标位置已有不同的会话，已拒绝覆盖".to_string())); }
        targets.push((target, content));
    }
    let mut imported = Vec::with_capacity(targets.len());
    for (target, content) in targets {
        if !target.exists() { if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; } crate::config::atomic_write(&target, content)?; }
        imported.push(parse_session(provider, &target)?.ok_or_else(|| AppError::Config("导入的会话内容无效".to_string()))?);
    }
    Ok(imported)
}

pub fn trash_session(provider: SessionProvider, source_path: &str) -> AppResult<SessionArchiveInfo> {
    let (source, relative) = validated_session(provider, source_path)?;
    let content = fs::read(&source)?;
    let meta = parse_session(provider, &source)?.ok_or_else(|| AppError::Config("无法读取会话元数据".to_string()))?;
    let manifest = session_manifest(provider, &meta, relative, &content);
    let dir = session_trash_dir(provider);
    fs::create_dir_all(&dir)?;
    let archive_path = dir.join(format!("{}-{}.zip", safe_session_name(&manifest.session_id), manifest.created_at));
    write_session_archive(&archive_path, &manifest, &content)?;
    fs::remove_file(source)?;
    Ok(SessionArchiveInfo { archive_path: archive_path.to_string_lossy().into_owned(), session_id: manifest.session_id, created_at: manifest.created_at })
}

pub fn restore_trashed_session(provider: SessionProvider, archive_path: &str) -> AppResult<SessionMeta> {
    let archive = Path::new(archive_path).canonicalize().map_err(|_| AppError::Config("找不到会话回收站归档".to_string()))?;
    let in_current_trash = session_trash_dir(provider).canonicalize().is_ok_and(|trash| archive.starts_with(trash));
    let in_legacy_claude_trash = provider == SessionProvider::ClaudeCode && config::get_app_config_dir()
        .join("session-trash").canonicalize().is_ok_and(|legacy| archive.starts_with(legacy));
    if (!in_current_trash && !in_legacy_claude_trash) || archive.extension().and_then(|value| value.to_str()) != Some("zip") { return Err(AppError::Path("只能恢复对应回收站中的会话归档".to_string())); }
    import_session(provider, &archive.to_string_lossy())
}

pub fn list_trashed_sessions(provider: SessionProvider) -> AppResult<Vec<SessionArchiveInfo>> {
    let mut archives = Vec::new();
    let mut directories = vec![session_trash_dir(provider)];
    if provider == SessionProvider::ClaudeCode { directories.push(config::get_app_config_dir().join("session-trash")); }
    for dir in directories {
        if !dir.is_dir() { continue; }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("zip") { continue; }
            if let Ok((manifest, _)) = read_session_archive(&path) {
                if manifest.provider == provider { archives.push(SessionArchiveInfo { archive_path: path.to_string_lossy().into_owned(), session_id: manifest.session_id, created_at: manifest.created_at }); }
            }
        }
    }
    archives.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(archives)
}


include!("claude_code.rs");
include!("pi.rs");
include!("dsh.rs");
include!("cline.rs");
include!("backup.rs");
include!("codex.rs");
include!("opencode.rs");
include!("tests.rs");
