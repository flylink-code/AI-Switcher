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

pub fn export_claude_code_session(
    source_path: &str,
    destination_dir: Option<&str>,
) -> AppResult<SessionArchiveInfo> {
    let (source, relative) = validated_code_session(source_path)?;
    let content = fs::read(&source)?;
    let meta = parse_claude_code_session(&source)?.ok_or_else(|| AppError::Config("无法读取会话元数据".to_string()))?;
    let manifest = SessionArchiveManifest {
        version: SESSION_ARCHIVE_VERSION,
        provider: SessionProvider::ClaudeCode,
        session_id: meta.session_id.clone(),
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        created_at: chrono::Utc::now().timestamp_millis(),
        content_sha256: hex::encode(Sha256::digest(&content)),
    };
    let dir = resolve_export_dir(destination_dir, "session-archives")?;
    let archive_path = dir.join(format!("{}-{}.zip", safe_session_name(&manifest.session_id), manifest.created_at));
    write_session_archive(&archive_path, &manifest, &content)?;
    Ok(SessionArchiveInfo { archive_path: archive_path.to_string_lossy().into_owned(), session_id: manifest.session_id, created_at: manifest.created_at })
}

/// Store each selected session as an independently restorable local archive.
/// The source session files are only read and remain untouched.
pub fn backup_claude_code_sessions(source_paths: &[String]) -> AppResult<SessionBatchBackupInfo> {
    let source_paths = unique_session_paths(source_paths)?;
    let mut archives = Vec::with_capacity(source_paths.len());
    for source_path in source_paths {
        archives.push(export_claude_code_session(&source_path, None)?);
    }
    Ok(SessionBatchBackupInfo { archives })
}

/// Create one portable ZIP containing every selected session and its integrity
/// metadata. Unlike the backup operation this produces a single file that can
/// be moved and imported on another machine.
pub fn export_claude_code_sessions(
    source_paths: &[String],
    destination_dir: Option<&str>,
) -> AppResult<SessionBatchExportInfo> {
    let source_paths = unique_session_paths(source_paths)?;
    let mut sessions = Vec::with_capacity(source_paths.len());
    for source_path in source_paths {
        let (source, relative) = validated_code_session(&source_path)?;
        let content = fs::read(&source)?;
        let meta = parse_claude_code_session(&source)?
            .ok_or_else(|| AppError::Config("无法读取会话元数据".to_string()))?;
        sessions.push((
            SessionArchiveManifest {
                version: SESSION_ARCHIVE_VERSION,
                provider: SessionProvider::ClaudeCode,
                session_id: meta.session_id,
                relative_path: relative.to_string_lossy().replace('\\', "/"),
                created_at: chrono::Utc::now().timestamp_millis(),
                content_sha256: hex::encode(Sha256::digest(&content)),
            },
            content,
        ));
    }

    let created_at = chrono::Utc::now().timestamp_millis();
    let dir = resolve_export_dir(destination_dir, "session-exports")?;
    let archive_path = dir.join(format!("claude-code-sessions-{created_at}.zip"));
    write_batch_session_archive(&archive_path, created_at, &sessions)?;
    Ok(SessionBatchExportInfo {
        archive_path: archive_path.to_string_lossy().into_owned(),
        session_count: sessions.len(),
        created_at,
    })
}

pub fn import_claude_code_session(archive_path: &str) -> AppResult<SessionMeta> {
    if is_batch_session_archive(Path::new(archive_path))? {
        let sessions = import_claude_code_sessions(archive_path)?;
        return sessions.into_iter().next().ok_or_else(|| AppError::Config("会话批量归档为空".to_string()));
    }
    let (manifest, content) = read_session_archive(Path::new(archive_path))?;
    let root = claude_code_session_root();
    let relative = safe_archive_relative_path(&manifest.relative_path)?;
    let target = root.join(relative);
    if target.exists() {
        let existing = fs::read(&target)?;
        if hex::encode(Sha256::digest(existing)) == manifest.content_sha256 {
            return parse_claude_code_session(&target)?.ok_or_else(|| AppError::Config("导入的会话内容无效".to_string()));
        }
        return Err(AppError::Config("目标位置已有不同的会话，已拒绝覆盖".to_string()));
    }
    if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
    crate::config::atomic_write(&target, &content)?;
    parse_claude_code_session(&target)?.ok_or_else(|| AppError::Config("导入的会话内容无效".to_string()))
}

pub fn import_claude_code_sessions(archive_path: &str) -> AppResult<Vec<SessionMeta>> {
    let (batch, contents) = read_batch_session_archive(Path::new(archive_path))?;
    let root = claude_code_session_root();
    let mut targets = Vec::with_capacity(batch.sessions.len());
    let mut target_paths = std::collections::HashSet::new();
    for (manifest, content) in batch.sessions.iter().zip(contents.iter()) {
        let relative = safe_archive_relative_path(&manifest.relative_path)?;
        let target = root.join(relative);
        if !target_paths.insert(target.clone()) {
            return Err(AppError::Config("会话批量归档包含重复的目标路径".to_string()));
        }
        if target.exists() {
            let existing = fs::read(&target)?;
            if hex::encode(Sha256::digest(existing)) != manifest.content_sha256 {
                return Err(AppError::Config("目标位置已有不同的会话，已拒绝覆盖".to_string()));
            }
        }
        targets.push((target, content));
    }

    let mut imported = Vec::with_capacity(targets.len());
    for (target, content) in targets {
        if !target.exists() {
            if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
            crate::config::atomic_write(&target, content)?;
        }
        imported.push(parse_claude_code_session(&target)?
            .ok_or_else(|| AppError::Config("导入的会话内容无效".to_string()))?);
    }
    Ok(imported)
}

pub fn trash_claude_code_session(source_path: &str) -> AppResult<SessionArchiveInfo> {
    let (source, relative) = validated_code_session(source_path)?;
    let content = fs::read(&source)?;
    let meta = parse_claude_code_session(&source)?.ok_or_else(|| AppError::Config("无法读取会话元数据".to_string()))?;
    let manifest = SessionArchiveManifest { version: SESSION_ARCHIVE_VERSION, provider: SessionProvider::ClaudeCode, session_id: meta.session_id.clone(), relative_path: relative.to_string_lossy().replace('\\', "/"), created_at: chrono::Utc::now().timestamp_millis(), content_sha256: hex::encode(Sha256::digest(&content)) };
    let dir = config::get_app_config_dir().join("session-trash");
    fs::create_dir_all(&dir)?;
    let archive_path = dir.join(format!("{}-{}.zip", safe_session_name(&manifest.session_id), manifest.created_at));
    write_session_archive(&archive_path, &manifest, &content)?;
    fs::remove_file(source)?;
    Ok(SessionArchiveInfo { archive_path: archive_path.to_string_lossy().into_owned(), session_id: manifest.session_id, created_at: manifest.created_at })
}

pub fn restore_trashed_claude_code_session(archive_path: &str) -> AppResult<SessionMeta> {
    let trash = config::get_app_config_dir().join("session-trash").canonicalize()
        .map_err(|_| AppError::Config("会话回收站为空".to_string()))?;
    let archive = Path::new(archive_path).canonicalize()
        .map_err(|_| AppError::Config("找不到会话回收站归档".to_string()))?;
    if !archive.starts_with(trash) || archive.extension().and_then(|value| value.to_str()) != Some("zip") {
        return Err(AppError::Path("只能恢复资料库回收站中的会话归档".to_string()));
    }
    import_claude_code_session(&archive.to_string_lossy())
}

pub fn list_trashed_claude_code_sessions() -> AppResult<Vec<SessionArchiveInfo>> {
    let dir = config::get_app_config_dir().join("session-trash");
    if !dir.is_dir() { return Ok(Vec::new()); }
    let mut archives = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("zip") { continue; }
        if let Ok((manifest, _)) = read_session_archive(&path) {
            archives.push(SessionArchiveInfo { archive_path: path.to_string_lossy().into_owned(), session_id: manifest.session_id, created_at: manifest.created_at });
        }
    }
    archives.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(archives)
}

fn scan_pi_sessions() -> (Vec<SessionMeta>, SessionProviderStatus) {
    let pi_dir = crate::coding::pi::config::get_pi_dir().join("sessions");
    let status_str = if pi_dir.exists() { "available" } else { "not_found" };
    let detail = if pi_dir.exists() {
        format!("发现 Pi 会话目录 ({})", pi_dir.display())
    } else {
        "未找到 Pi 会话目录".to_string()
    };
    let status = SessionProviderStatus {
        provider: SessionProvider::Pi,
        status: status_str.to_string(),
        detail,
        root_path: Some(pi_dir.to_string_lossy().into_owned()),
    };

    let items = match crate::coding::pi::session::scan_pi_sessions_sync() {
        Ok(items) => items,
        Err(_) => Vec::new(),
    };

    let metas = items.into_iter().map(|item| SessionMeta {
        provider: SessionProvider::Pi,
        session_id: item.id.clone(),
        title: item.title,
        summary: item.model.map(|m| format!("Model: {m}")),
        project_dir: None,
        created_at: item.created_at.map(|s| s as i64 * 1000),
        last_active_at: item.updated_at.map(|s| s as i64 * 1000),
        source_path: item.file_path,
        resume_command: Some(format!("pi --resume {}", item.id)),
        pinned: false,
    }).collect();

    (metas, status)
}

fn session_root(provider: SessionProvider) -> AppResult<PathBuf> {
    match provider {
        SessionProvider::ClaudeCode => Ok(claude_code_session_root()),
        SessionProvider::Codex => Ok(codex_session_root()),
        SessionProvider::OpenCode => Err(AppError::Config(
            "OpenCode 会话暂不支持归档、回收站与导入操作".to_string(),
        )),
        SessionProvider::Pi => Ok(crate::coding::pi::config::get_pi_dir().join("sessions")),
    }
}

fn parse_session(provider: SessionProvider, path: &Path) -> AppResult<Option<SessionMeta>> {
    match provider {
        SessionProvider::ClaudeCode => parse_claude_code_session(path),
        SessionProvider::Codex => parse_codex_session(path),
        SessionProvider::OpenCode => Ok(None),
        SessionProvider::Pi => {
            let mtime = path
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or(0);
            Ok(session_meta_from_path(SessionProvider::Pi, path, mtime))
        }
    }
}

fn validated_session(provider: SessionProvider, source_path: &str) -> AppResult<(PathBuf, PathBuf)> {
    let root = session_root(provider)?;
    let source = validate_session_path_in_root(&root, Path::new(source_path))?;
    let root = root.canonicalize()?;
    let relative = source.strip_prefix(root).map_err(|_| AppError::Path("会话相对路径无效".to_string()))?.to_path_buf();
    Ok((source, relative))
}

fn session_manifest(provider: SessionProvider, meta: &SessionMeta, relative: PathBuf, content: &[u8]) -> SessionArchiveManifest {
    SessionArchiveManifest {
        version: SESSION_ARCHIVE_VERSION,
        provider,
        session_id: meta.session_id.clone(),
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        created_at: chrono::Utc::now().timestamp_millis(),
        content_sha256: hex::encode(Sha256::digest(content)),
    }
}

fn validate_manifest_provider(provider: SessionProvider, manifest: &SessionArchiveManifest) -> AppResult<()> {
    if manifest.provider != provider {
        return Err(AppError::Config("会话归档来源与目标不匹配".to_string()));
    }
    Ok(())
}

fn import_target(provider: SessionProvider, relative_path: &str) -> AppResult<PathBuf> {
    let relative = safe_archive_relative_path(relative_path)?;
    let root = session_root(provider)?;
    fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let mut current = root.clone();
    for component in relative.components() {
        current.push(component.as_os_str());
        if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(AppError::Path("会话导入路径不能穿过符号链接".to_string()));
        }
    }
    Ok(root.join(relative))
}

fn session_trash_dir(provider: SessionProvider) -> PathBuf {
    let target = match provider {
        SessionProvider::Codex => "codex",
        SessionProvider::OpenCode => "opencode",
        SessionProvider::Pi => "pi",
        _ => "claude-code",
    };
    config::get_app_config_dir().join("session-trash").join(target)
}

fn validated_code_session(source_path: &str) -> AppResult<(PathBuf, PathBuf)> {
    let root = claude_code_session_root();
    let source = validate_session_path_in_root(&root, Path::new(source_path))?;
    let root = root.canonicalize()?;
    let relative = source.strip_prefix(root).map_err(|_| AppError::Path("会话相对路径无效".to_string()))?.to_path_buf();
    Ok((source, relative))
}

fn write_session_archive(path: &Path, manifest: &SessionArchiveManifest, content: &[u8]) -> AppResult<()> {
    let file = File::create(path)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file(SESSION_ARCHIVE_MANIFEST, options).map_err(|error| AppError::Other(format!("创建会话归档失败: {error}")))?;
    archive.write_all(&serde_json::to_vec_pretty(manifest)?)?;
    archive.start_file(SESSION_ARCHIVE_CONTENT, options).map_err(|error| AppError::Other(format!("创建会话归档失败: {error}")))?;
    archive.write_all(content)?;
    archive.finish().map_err(|error| AppError::Other(format!("完成会话归档失败: {error}")))?;
    Ok(())
}

fn write_batch_session_archive(
    path: &Path,
    created_at: i64,
    sessions: &[(SessionArchiveManifest, Vec<u8>)],
) -> AppResult<()> {
    let manifest = SessionBatchArchiveManifest {
        version: SESSION_BATCH_ARCHIVE_VERSION,
        created_at,
        sessions: sessions.iter().map(|(manifest, _)| manifest.clone()).collect(),
    };
    let file = File::create(path)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file(SESSION_BATCH_ARCHIVE_MANIFEST, options)
        .map_err(|error| AppError::Other(format!("创建会话批量归档失败: {error}")))?;
    archive.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for (index, (_, content)) in sessions.iter().enumerate() {
        archive.start_file(format!("{SESSION_BATCH_ARCHIVE_PREFIX}/{index}/session.jsonl"), options)
            .map_err(|error| AppError::Other(format!("创建会话批量归档失败: {error}")))?;
        archive.write_all(content)?;
    }
    archive.finish().map_err(|error| AppError::Other(format!("完成会话批量归档失败: {error}")))?;
    Ok(())
}

fn read_session_archive(path: &Path) -> AppResult<(SessionArchiveManifest, Vec<u8>)> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|_| AppError::Config("会话归档格式无效".to_string()))?;
    let mut manifest = Vec::new();
    archive.by_name(SESSION_ARCHIVE_MANIFEST).map_err(|_| AppError::Config("会话归档缺少清单".to_string()))?.read_to_end(&mut manifest)?;
    let manifest = serde_json::from_slice::<SessionArchiveManifest>(&manifest)?;
    if manifest.version != SESSION_ARCHIVE_VERSION { return Err(AppError::Config("不支持的会话归档版本".to_string())); }
    let mut content = Vec::new();
    archive.by_name(SESSION_ARCHIVE_CONTENT).map_err(|_| AppError::Config("会话归档缺少内容".to_string()))?.read_to_end(&mut content)?;
    if hex::encode(Sha256::digest(&content)) != manifest.content_sha256 { return Err(AppError::Config("会话归档校验失败".to_string())); }
    Ok((manifest, content))
}

fn is_batch_session_archive(path: &Path) -> AppResult<bool> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|_| AppError::Config("会话归档格式无效".to_string()))?;
    let contains_batch_manifest = archive.by_name(SESSION_BATCH_ARCHIVE_MANIFEST).is_ok();
    Ok(contains_batch_manifest)
}

fn read_batch_session_archive(path: &Path) -> AppResult<(SessionBatchArchiveManifest, Vec<Vec<u8>>)> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|_| AppError::Config("会话归档格式无效".to_string()))?;
    let mut raw_manifest = Vec::new();
    archive.by_name(SESSION_BATCH_ARCHIVE_MANIFEST)
        .map_err(|_| AppError::Config("会话批量归档缺少清单".to_string()))?
        .read_to_end(&mut raw_manifest)?;
    let manifest = serde_json::from_slice::<SessionBatchArchiveManifest>(&raw_manifest)?;
    if manifest.version != SESSION_BATCH_ARCHIVE_VERSION || manifest.sessions.is_empty() {
        return Err(AppError::Config("不支持或为空的会话批量归档".to_string()));
    }
    let mut contents = Vec::with_capacity(manifest.sessions.len());
    for (index, item) in manifest.sessions.iter().enumerate() {
        if item.version != SESSION_ARCHIVE_VERSION { return Err(AppError::Config("会话批量归档包含不支持的会话版本".to_string())); }
        let mut content = Vec::new();
        archive.by_name(&format!("{SESSION_BATCH_ARCHIVE_PREFIX}/{index}/session.jsonl"))
            .map_err(|_| AppError::Config("会话批量归档缺少内容".to_string()))?
            .read_to_end(&mut content)?;
        if hex::encode(Sha256::digest(&content)) != item.content_sha256 {
            return Err(AppError::Config("会话批量归档校验失败".to_string()));
        }
        contents.push(content);
    }
    Ok((manifest, contents))
}

fn unique_session_paths(source_paths: &[String]) -> AppResult<Vec<String>> {
    let mut unique = std::collections::BTreeSet::new();
    for path in source_paths {
        if !path.trim().is_empty() { unique.insert(path.clone()); }
    }
    if unique.is_empty() { return Err(AppError::Config("请至少选择一个 Claude Code 会话".to_string())); }
    Ok(unique.into_iter().collect())
}

/// Resolve a user-selected directory for portable exports. The default keeps
/// backwards compatibility with previous releases, while explicit paths must
/// already be directories so an arbitrary file path can never be overwritten.
fn resolve_export_dir(destination_dir: Option<&str>, default_subdir: &str) -> AppResult<PathBuf> {
    match destination_dir.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            let metadata = fs::metadata(&path).map_err(|error| {
                AppError::Path(format!("无法访问导出目录 {}: {error}", path.display()))
            })?;
            if !metadata.is_dir() {
                return Err(AppError::Path(format!("导出位置不是目录: {}", path.display())));
            }
            path.canonicalize().map_err(|error| {
                AppError::Path(format!("无法解析导出目录 {}: {error}", path.display()))
            })
        }
        None => {
            let dir = config::get_app_config_dir().join(default_subdir);
            fs::create_dir_all(&dir)?;
            Ok(dir)
        }
    }
}

fn safe_archive_relative_path(value: &str) -> AppResult<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute() || path.components().any(|component| matches!(component, std::path::Component::ParentDir | std::path::Component::Prefix(_) | std::path::Component::RootDir)) || path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
        return Err(AppError::Config("会话归档中的路径不安全".to_string()));
    }
    Ok(path.to_path_buf())
}

fn safe_session_name(value: &str) -> String { value.chars().filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')).collect::<String>() }

fn collect_claude_code_session_paths(
) -> AppResult<(Vec<(PathBuf, i64)>, SessionProviderStatus, bool, bool)> {
    let root = claude_code_session_root();
    if !root.is_dir() {
        return Ok((
            Vec::new(),
            SessionProviderStatus {
                provider: SessionProvider::ClaudeCode,
                status: "not_found".to_string(),
                detail: "未发现 Claude Code 本地会话目录".to_string(),
                root_path: Some(root.display().to_string()),
            },
            false,
            false,
        ));
    }
    let mut paths = Vec::new();
    let deadline = Instant::now() + WALK_DEADLINE;
    let (truncated, timed_out) = collect_jsonl_files_with_mtime(&root, &mut paths, 0, deadline)?;
    Ok((
        paths,
        SessionProviderStatus {
            provider: SessionProvider::ClaudeCode,
            status: "available".to_string(),
            detail: "Claude Code 本地会话可用".to_string(),
            root_path: Some(root.display().to_string()),
        },
        truncated,
        timed_out,
    ))
}

fn collect_codex_session_paths() -> AppResult<(Vec<(PathBuf, i64)>, SessionProviderStatus, bool, bool)> {
    let first = collect_codex_session_paths_once()?;
    if !first.0.is_empty() || first.1.status == "not_found" {
        return Ok(first);
    }
    // Post-update / antivirus settle: first walk can briefly see an empty tree
    // even when ~/.codex/sessions exists on disk.
    log::warn!(
        "Codex 会话首次扫描为空（status={} detail={}），500ms 后重试",
        first.1.status,
        first.1.detail
    );
    std::thread::sleep(Duration::from_millis(500));
    let second = collect_codex_session_paths_once()?;
    if !second.0.is_empty() {
        return Ok(second);
    }
    std::thread::sleep(Duration::from_millis(1_500));
    let third = collect_codex_session_paths_once()?;
    log::info!(
        "Codex 会话扫描结束: count={} status={} root={:?}",
        third.0.len(),
        third.1.status,
        third.1.root_path
    );
    Ok(third)
}

fn collect_codex_session_paths_once() -> AppResult<(Vec<(PathBuf, i64)>, SessionProviderStatus, bool, bool)> {
    let root = codex_session_root();
    let archived = codex_archived_session_root();
    if !root.is_dir() && !archived.is_dir() {
        // Directory missing — still try SQLite rollout paths (custom CODEX_HOME layouts).
        let mut paths = Vec::new();
        merge_codex_sqlite_rollout_paths(&mut paths);
        if paths.is_empty() {
            // A junction/symlink root with an unreachable target (drive offline)
            // reports NotFound too — but that is transient, not "no sessions".
            let (status, detail) = match config::broken_link_note(&root) {
                Some(note) => (
                    "degraded".to_string(),
                    format!("Codex 会话目录暂不可达：{note}"),
                ),
                None => (
                    "not_found".to_string(),
                    "未发现 Codex 本地会话目录".to_string(),
                ),
            };
            return Ok((
                Vec::new(),
                SessionProviderStatus {
                    provider: SessionProvider::Codex,
                    status,
                    detail,
                    root_path: Some(root.display().to_string()),
                },
                false,
                false,
            ));
        }
        return Ok((
            paths,
            SessionProviderStatus {
                provider: SessionProvider::Codex,
                status: "available".to_string(),
                detail: "Codex 会话由 SQLite rollout 路径兜底列出".to_string(),
                root_path: Some(root.display().to_string()),
            },
            false,
            false,
        ));
    }
    let mut paths = Vec::new();
    let deadline = Instant::now() + WALK_DEADLINE;
    let mut truncated = false;
    let mut timed_out = false;
    if root.is_dir() {
        let (part_truncated, part_timed_out) =
            collect_jsonl_files_with_mtime(&root, &mut paths, 0, deadline)?;
        truncated |= part_truncated;
        timed_out |= part_timed_out;
    }
    if archived.is_dir() && Instant::now() < deadline {
        let (part_truncated, part_timed_out) =
            collect_jsonl_files_with_mtime(&archived, &mut paths, 0, deadline)?;
        truncated |= part_truncated;
        timed_out |= part_timed_out;
    }
    // Always merge SQLite rollout paths so locked/partial walks cannot hide sessions.
    merge_codex_sqlite_rollout_paths(&mut paths);
    let (status, detail) = if paths.is_empty() {
        (
            "degraded".to_string(),
            format!(
                "会话目录存在但未扫到 jsonl（可能被杀毒/云同步短暂锁住）：{}",
                root.display()
            ),
        )
    } else if timed_out {
        (
            "available".to_string(),
            "Codex 本地会话可用（目录扫描超时，已合并 SQLite / 归档索引）".to_string(),
        )
    } else {
        (
            "available".to_string(),
            "Codex 本地会话可用".to_string(),
        )
    };
    Ok((
        paths,
        SessionProviderStatus {
            provider: SessionProvider::Codex,
            status,
            detail,
            root_path: Some(root.display().to_string()),
        },
        truncated,
        timed_out,
    ))
}

/// Append existing rollout JSONL paths from Codex thread DBs that are not already listed.
fn merge_codex_sqlite_rollout_paths(paths: &mut Vec<(PathBuf, i64)>) {
    let mut seen: HashSet<String> = paths
        .iter()
        .map(|(path, _)| normalize_path_key(path))
        .collect();
    for db_path in codex_thread_db_paths() {
        let Ok(db) = Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        if !sqlite_table_exists(&db, "threads") || !sqlite_column_exists(&db, "threads", "rollout_path") {
            continue;
        }
        let has_updated = sqlite_column_exists(&db, "threads", "updated_at_ms");
        let sql = if has_updated {
            "SELECT rollout_path, updated_at_ms FROM threads WHERE rollout_path IS NOT NULL AND TRIM(rollout_path) <> ''"
        } else {
            "SELECT rollout_path, NULL FROM threads WHERE rollout_path IS NOT NULL AND TRIM(rollout_path) <> ''"
        };
        let Ok(mut stmt) = db.prepare(sql) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let updated: Option<i64> = row.get(1)?;
            Ok((path, updated))
        }) else {
            continue;
        };
        for row in rows.flatten() {
            let (raw, updated) = row;
            let path = PathBuf::from(strip_windows_path_prefix(raw.trim()));
            if !path.is_file() {
                continue;
            }
            let key = normalize_path_key(&path);
            if !seen.insert(key) {
                continue;
            }
            if paths.len() >= MAX_SESSION_FILES {
                break;
            }
            let mtime = updated.unwrap_or_else(|| {
                path.metadata()
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| {
                        time.duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_millis() as i64)
                    })
                    .unwrap_or(0)
            });
            paths.push((path, mtime));
        }
    }
}

fn claude_code_session_root() -> PathBuf {
    config::get_claude_config_dir().join("projects")
}

fn codex_session_root() -> PathBuf {
    config::get_codex_config_dir().join("sessions")
}

fn codex_archived_session_root() -> PathBuf {
    config::get_codex_config_dir().join("archived_sessions")
}


fn collect_jsonl_files(directory: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
    let mut with_mtime = Vec::new();
    let deadline = Instant::now() + WALK_DEADLINE;
    let _ = collect_jsonl_files_with_mtime(directory, &mut with_mtime, 0, deadline)?;
    files.extend(with_mtime.into_iter().map(|(path, _)| path));
    Ok(())
}

/// Walk session trees using DirEntry metadata only (never open file contents).
/// Returns `(truncated_by_count, timed_out)`.
fn collect_jsonl_files_with_mtime(
    directory: &Path,
    files: &mut Vec<(PathBuf, i64)>,
    depth: u32,
    deadline: Instant,
) -> AppResult<(bool, bool)> {
    if files.len() >= MAX_SESSION_FILES {
        return Ok((true, false));
    }
    if depth > MAX_WALK_DEPTH {
        return Ok((false, false));
    }
    if Instant::now() >= deadline {
        return Ok((false, true));
    }
    let entries = match fs::read_dir(directory) {
        Ok(value) => value,
        Err(error) => {
            log::warn!("跳过无法读取的会话目录 {}: {error}", directory.display());
            return Ok((false, false));
        }
    };
    let mut timed_out = false;
    let mut truncated = false;
    for entry in entries {
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        if files.len() >= MAX_SESSION_FILES {
            truncated = true;
            break;
        }
        let entry = match entry {
            Ok(value) => value,
            Err(error) => {
                log::warn!("跳过无法读取的会话目录项: {error}");
                continue;
            }
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let metadata = entry.metadata().ok();
        // This list view never opens JSONL contents. A recall/offline
        // attribute therefore must not hide an otherwise valid local session:
        // it made existing Codex histories disappear in the UI while the
        // usage scanner could still see the same files. Opening content stays
        // deferred until the user selects a row.
        if file_type.is_dir() {
            let (child_truncated, child_timeout) =
                collect_jsonl_files_with_mtime(&path, files, depth + 1, deadline)?;
            truncated |= child_truncated;
            if child_timeout {
                timed_out = true;
                break;
            }
            continue;
        }
        let is_jsonl = path.extension().and_then(|value| value.to_str()) == Some("jsonl");
        let is_agent = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("agent-"));
        if is_jsonl && !is_agent {
            let mtime = metadata
                .as_ref()
                .and_then(|value| value.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or(0);
            files.push((path, mtime));
        }
    }
    Ok((truncated, timed_out))
}

/// Build list-row metadata without opening the jsonl (avoids OS freezes).
fn session_meta_from_path(
    provider: SessionProvider,
    path: &Path,
    mtime: i64,
) -> Option<SessionMeta> {
    let session_id = path.file_stem()?.to_str()?.to_string();
    if session_id.is_empty() {
        return None;
    }
    let project_dir = match provider {
        SessionProvider::ClaudeCode => path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(|name| name.replace('-', "/")),
        SessionProvider::Codex | SessionProvider::OpenCode | SessionProvider::Pi => None,
    };
    let resume = match provider {
        SessionProvider::ClaudeCode => resume_command(&session_id),
        SessionProvider::Codex => Some(format!("codex resume {session_id}")),
        // OpenCode / Pi 元数据走 Materialized 路径，不会经过这里。
        SessionProvider::OpenCode => Some(format!("opencode -s {session_id}")),
        SessionProvider::Pi => Some(format!("pi --resume {session_id}")),
    };
    Some(SessionMeta {
        provider,
        session_id: session_id.clone(),
        title: Some(session_id.clone()),
        summary: None,
        project_dir,
        created_at: None,
        last_active_at: (mtime > 0).then_some(mtime),
        source_path: path.display().to_string(),
        resume_command: resume,
        pinned: false,
    })
}

fn open_session_file(path: &Path) -> AppResult<File> {
    // Direct open only — never spawn abandoned timeout threads (those can strand
    // kernel waits and help freeze Windows under OneDrive/AV pressure).
    File::open(path).map_err(|error| {
        AppError::Io(format!("打开会话 {} 失败: {error}", path.display()))
    })
}

fn parse_claude_code_session(path: &Path) -> AppResult<Option<SessionMeta>> {
    let file = open_session_file(path)?;
    let reader = BufReader::new(file);
    let mut session_id = None;
    let mut project_dir = None;
    let mut created_at = None;
    let mut first_user_message = None;

    for line in reader.lines().take(60) {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        session_id = session_id.or_else(|| {
            value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        project_dir = project_dir.or_else(|| {
            value
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        created_at = created_at.or_else(|| value.get("timestamp").and_then(parse_timestamp));

        if first_user_message.is_none() && message_role(&value).as_deref() == Some("user") {
            let content = message_content(&value);
            let trimmed = content.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('/') {
                first_user_message = Some(truncate(trimmed, SUMMARY_LIMIT));
            }
        }
    }

    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_string)
    });
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let last_active_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64);

    Ok(Some(SessionMeta {
        provider: SessionProvider::ClaudeCode,
        session_id: session_id.clone(),
        title: first_user_message.clone(),
        summary: first_user_message,
        project_dir,
        created_at,
        last_active_at,
        source_path: path.display().to_string(),
        resume_command: resume_command(&session_id),
        pinned: false,
    }))
}

fn parse_codex_session(path: &Path) -> AppResult<Option<SessionMeta>> {
    let mut session = parse_claude_code_session(path)?;
    if let Some(session) = &mut session {
        session.provider = SessionProvider::Codex;
        session.resume_command = Some(format!("codex resume {}", session.session_id));
        let index = load_codex_thread_index();
        apply_codex_thread_meta(session, path, &index);
    }
    Ok(session)
}

fn load_claude_code_messages(path: &Path) -> AppResult<Vec<SessionMessage>> {
    let file = open_session_file(path)?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(role) = message_role(&value) else {
            continue;
        };
        let content = message_content(&value);
        if content.trim().is_empty() {
            continue;
        }
        messages.push(SessionMessage {
            role,
            content,
            timestamp: value.get("timestamp").and_then(parse_timestamp),
        });
    }

    Ok(messages)
}

/// Codex rollout JSONL uses `response_item` / `event_msg`, not Claude Code's `message` envelope.
fn load_codex_messages(path: &Path) -> AppResult<Vec<SessionMessage>> {
    let file = open_session_file(path)?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let timestamp = value.get("timestamp").and_then(parse_timestamp);
        match value.get("type").and_then(Value::as_str) {
            Some("response_item") => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                let Some((role, content)) = codex_response_item_message(payload) else {
                    continue;
                };
                if content.trim().is_empty() {
                    continue;
                }
                messages.push(SessionMessage {
                    role,
                    content,
                    timestamp,
                });
            }
            Some("event_msg") => {
                // Prefer response_item for chat turns; keep agent_message only as fallback
                // when it carries visible assistant text without a paired response_item.
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(Value::as_str) != Some("agent_message") {
                    continue;
                }
                let content = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if content.is_empty() {
                    continue;
                }
                // Skip if the same text was already captured from response_item.
                if messages.iter().any(|item| item.role == "assistant" && item.content == content) {
                    continue;
                }
                messages.push(SessionMessage {
                    role: "assistant".to_string(),
                    content,
                    timestamp,
                });
            }
            _ => {}
        }
    }

    Ok(messages)
}

fn codex_response_item_message(payload: &Value) -> Option<(String, String)> {
    match payload.get("type").and_then(Value::as_str)? {
        "message" => {
            let role = payload.get("role")?.as_str()?;
            if matches!(role, "developer" | "system") {
                return None;
            }
            let content = extract_text(payload.get("content").unwrap_or(&Value::Null));
            Some((role.to_string(), content))
        }
        "custom_tool_call" | "function_call" => {
            let name = payload
                .get("name")
                .or_else(|| payload.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = payload
                .get("input")
                .or_else(|| payload.get("arguments"))
                .map(extract_text)
                .unwrap_or_default();
            let content = if args.trim().is_empty() {
                format!("[tool: {name}]")
            } else {
                format!("[tool: {name}]\n{}", truncate(&args, SUMMARY_LIMIT * 4))
            };
            Some(("tool".to_string(), content))
        }
        "custom_tool_call_output" | "function_call_output" => {
            let content = payload
                .get("output")
                .or_else(|| payload.get("content"))
                .map(extract_text)
                .unwrap_or_default();
            if content.trim().is_empty() {
                None
            } else {
                Some(("tool".to_string(), truncate(&content, SUMMARY_LIMIT * 4)))
            }
        }
        _ => None,
    }
}

fn message_role(value: &Value) -> Option<String> {
    let message = value.get("message")?;
    let mut role = message.get("role")?.as_str()?.to_string();
    if role == "user" {
        if let Some(items) = message.get("content").and_then(Value::as_array) {
            if !items.is_empty()
                && items.iter().all(|item| {
                    item.get("type").and_then(Value::as_str) == Some("tool_result")
                })
            {
                role = "tool".to_string();
            }
        }
    }
    Some(role)
}

fn message_content(value: &Value) -> String {
    value
        .get("message")
        .and_then(|message| message.get("content"))
        .map(extract_text)
        .unwrap_or_default()
}

fn extract_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(extract_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(content) = map.get("content") {
                return extract_text(content);
            }
            if map.get("type").and_then(Value::as_str) == Some("tool_use") {
                let name = map.get("name").and_then(Value::as_str).unwrap_or("tool");
                return format!("[tool: {name}]");
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn parse_timestamp(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(if number < 10_000_000_000 {
            number * 1000
        } else {
            number
        });
    }
    value
        .as_str()
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|value| value.timestamp_millis())
}

fn session_metadata_contains(session: &SessionMeta, query: &str) -> bool {
    [
        Some(session.session_id.as_str()),
        session.title.as_deref(),
        session.summary.as_deref(),
        session.project_dir.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(query))
        || (session.pinned && ("pin" == query || "pinned" == query || "置顶" == query))
}

fn file_contains(provider: SessionProvider, path: &str, query: &str) -> AppResult<bool> {
    // OpenCode 会话不在独立 .jsonl 文件里（SQLite 行 / storage 目录），
    // 内容搜索只匹配元数据。
    if provider == SessionProvider::OpenCode {
        return Ok(false);
    }
    let root = session_root(provider)?;
    let source = validate_session_path_in_root(&root, Path::new(path))?;
    let file = File::open(&source)
        .map_err(|error| AppError::Io(format!("打开会话 {} 失败: {error}", source.display())))?;
    for line in BufReader::new(file).lines() {
        if line
            .ok()
            .is_some_and(|value| value.to_lowercase().contains(query))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_session_path_in_root(root: &Path, source: &Path) -> AppResult<PathBuf> {
    if source.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return Err(AppError::Path(format!(
            "会话文件不在允许的目录内: {}",
            source.display()
        )));
    }
    let source = PathBuf::from(strip_windows_path_prefix(&source.to_string_lossy()));
    let candidate = if source.is_absolute() {
        source
    } else {
        root.join(source)
    };
    if candidate.extension().and_then(|value| value.to_str()) != Some("jsonl") {
        return Err(AppError::Path(format!(
            "会话文件不在允许的目录内: {}",
            candidate.display()
        )));
    }
    // Prefer prefix checks without canonicalize — canonicalize can hang on cloud FS.
    let root_key = normalize_path_key(root);
    let candidate_key = normalize_path_key(&candidate);
    if candidate_key.starts_with(&root_key)
        && (candidate_key.len() == root_key.len()
            || candidate_key.as_bytes().get(root_key.len()) == Some(&b'\\')
            || candidate_key.as_bytes().get(root_key.len()) == Some(&b'/'))
    {
        return Ok(candidate);
    }
    // Codex may keep historical rollouts under a previous CODEX_HOME; allow those
    // when they still exist and live under a `sessions` / `archived_sessions` tree.
    if candidate.is_file()
        && (candidate_key.contains(r"\sessions\")
            || candidate_key.contains(r"\archived_sessions\")
            || candidate_key.contains("/sessions/")
            || candidate_key.contains("/archived_sessions/"))
    {
        return Ok(candidate);
    }
    let root = root.canonicalize().map_err(|error| {
        AppError::Path(format!("无法解析会话根目录 {}: {error}", root.display()))
    })?;
    let source = candidate.canonicalize().map_err(|error| {
        AppError::Path(format!("无法解析会话文件 {}: {error}", candidate.display()))
    })?;
    if !source.starts_with(&root) {
        return Err(AppError::Path(format!(
            "会话文件不在允许的目录内: {}",
            source.display()
        )));
    }
    Ok(source)
}

fn normalize_path_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = strip_windows_path_prefix(raw.as_ref());
    stripped
        .replace('/', "\\")
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn strip_windows_path_prefix(path: &str) -> &str {
    let trimmed = path.trim();
    trimmed
        .strip_prefix(r"\\?\")
        .or_else(|| trimmed.strip_prefix(r"//?/"))
        .unwrap_or(trimmed)
}

fn normalize_cwd_display(cwd: &str) -> String {
    strip_windows_path_prefix(cwd.trim()).replace('/', "\\")
}

#[derive(Debug, Clone, Default)]
struct CodexThreadMeta {
    id: String,
    title: Option<String>,
    name: Option<String>,
    summary: Option<String>,
    cwd: Option<String>,
    pinned: bool,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

#[derive(Debug, Default)]
struct CodexThreadIndex {
    by_path: HashMap<String, CodexThreadMeta>,
    by_id: HashMap<String, CodexThreadMeta>,
}

impl CodexThreadIndex {
    fn lookup(&self, path: &Path) -> Option<&CodexThreadMeta> {
        let key = normalize_path_key(path);
        if let Some(meta) = self.by_path.get(&key) {
            return Some(meta);
        }
        let file_name = path.file_name().and_then(|value| value.to_str())?;
        for (id, meta) in &self.by_id {
            if file_name.contains(id) {
                return Some(meta);
            }
        }
        None
    }
}

fn apply_codex_thread_meta(session: &mut SessionMeta, path: &Path, index: &CodexThreadIndex) {
    let Some(meta) = index.lookup(path) else {
        return;
    };
    if !meta.id.is_empty() {
        session.session_id = meta.id.clone();
        session.resume_command = Some(format!("codex resume {}", meta.id));
    }
    let display = meta
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            meta.title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            meta.summary
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    if let Some(title) = display {
        session.title = Some(truncate(title, SUMMARY_LIMIT));
        if session.summary.is_none() {
            session.summary = Some(truncate(title, SUMMARY_LIMIT));
        }
    }
    if session.project_dir.is_none() {
        if let Some(cwd) = meta
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            session.project_dir = Some(normalize_cwd_display(cwd));
        }
    }
    if session.created_at.is_none() {
        session.created_at = meta.created_at;
    }
    if let Some(updated_at) = meta.updated_at {
        session.last_active_at = Some(updated_at);
    }
    session.pinned = meta.pinned;
}

fn load_codex_thread_index() -> CodexThreadIndex {
    let mut index = CodexThreadIndex::default();
    for path in codex_thread_db_paths() {
        if let Err(error) = load_codex_thread_index_from_db(&path, &mut index) {
            log::warn!(
                "跳过无法读取的 Codex 会话索引 {}: {error}",
                path.display()
            );
        }
    }
    index
}

fn codex_thread_db_paths() -> Vec<PathBuf> {
    let home = config::get_codex_config_dir();
    let mut paths = Vec::new();
    let legacy = home.join("state_5.sqlite");
    if legacy.is_file() {
        paths.push(legacy);
    }
    if let Ok(entries) = fs::read_dir(home.join("sqlite")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !(name.ends_with(".sqlite") || name.ends_with(".db")) {
                continue;
            }
            if name.ends_with("-wal") || name.ends_with("-shm") {
                continue;
            }
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn load_codex_thread_index_from_db(path: &Path, index: &mut CodexThreadIndex) -> AppResult<()> {
    let db = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| {
        AppError::Database(format!("打开 Codex SQLite 失败 {}: {error}", path.display()))
    })?;
    if !sqlite_table_exists(&db, "threads") {
        return Ok(());
    }
    let has_id = sqlite_column_exists(&db, "threads", "id");
    let has_rollout = sqlite_column_exists(&db, "threads", "rollout_path");
    if !has_id && !has_rollout {
        return Ok(());
    }
    let has_title = sqlite_column_exists(&db, "threads", "title");
    let has_name = sqlite_column_exists(&db, "threads", "name");
    let has_preview = sqlite_column_exists(&db, "threads", "preview");
    let has_first = sqlite_column_exists(&db, "threads", "first_user_message");
    let has_cwd = sqlite_column_exists(&db, "threads", "cwd");
    let has_pinned = sqlite_column_exists(&db, "threads", "is_pinned");
    let has_created = sqlite_column_exists(&db, "threads", "created_at_ms");
    let has_updated = sqlite_column_exists(&db, "threads", "updated_at_ms");

    let mut columns = Vec::new();
    if has_id {
        columns.push("id");
    }
    if has_rollout {
        columns.push("rollout_path");
    }
    if has_title {
        columns.push("title");
    }
    if has_name {
        columns.push("name");
    }
    if has_preview {
        columns.push("preview");
    }
    if has_first {
        columns.push("first_user_message");
    }
    if has_cwd {
        columns.push("cwd");
    }
    if has_pinned {
        columns.push("is_pinned");
    }
    if has_created {
        columns.push("created_at_ms");
    }
    if has_updated {
        columns.push("updated_at_ms");
    }
    let sql = format!("SELECT {} FROM threads", columns.join(", "));
    let mut stmt = db.prepare(&sql).map_err(|error| {
        AppError::Database(format!("查询 Codex threads 失败: {error}"))
    })?;
    let rows = stmt
        .query_map([], |row| {
            let mut offset = 0usize;
            let mut next = || {
                let value = offset;
                offset += 1;
                value
            };
            let id = if has_id {
                row.get::<_, Option<String>>(next())?
            } else {
                None
            };
            let rollout_path = if has_rollout {
                row.get::<_, Option<String>>(next())?
            } else {
                None
            };
            let title = if has_title {
                row.get::<_, Option<String>>(next())?
            } else {
                None
            };
            let name = if has_name {
                row.get::<_, Option<String>>(next())?
            } else {
                None
            };
            let preview = if has_preview {
                row.get::<_, Option<String>>(next())?
            } else {
                None
            };
            let first_user_message = if has_first {
                row.get::<_, Option<String>>(next())?
            } else {
                None
            };
            let cwd = if has_cwd {
                row.get::<_, Option<String>>(next())?
            } else {
                None
            };
            let pinned = if has_pinned {
                row.get::<_, Option<i64>>(next())?.unwrap_or(0) != 0
            } else {
                false
            };
            let created_at = if has_created {
                row.get::<_, Option<i64>>(next())?
            } else {
                None
            };
            let updated_at = if has_updated {
                row.get::<_, Option<i64>>(next())?
            } else {
                None
            };
            Ok((
                rollout_path,
                CodexThreadMeta {
                    id: id.unwrap_or_default(),
                    title: nonempty_owned(title),
                    name: nonempty_owned(name),
                    summary: nonempty_owned(preview).or_else(|| nonempty_owned(first_user_message)),
                    cwd: nonempty_owned(cwd),
                    pinned,
                    created_at,
                    updated_at,
                },
            ))
        })
        .map_err(|error| AppError::Database(format!("读取 Codex threads 失败: {error}")))?;

    for (rollout_path, meta) in rows.flatten() {
        if !meta.id.is_empty() {
            index.by_id.entry(meta.id.clone()).or_insert_with(|| meta.clone());
        }
        if let Some(path) = rollout_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            index
                .by_path
                .entry(normalize_path_key(Path::new(path)))
                .or_insert(meta);
        }
    }
    Ok(())
}

fn nonempty_owned(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn sqlite_table_exists(db: &Connection, table: &str) -> bool {
    db.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1")
        .ok()
        .and_then(|mut stmt| stmt.exists([table]).ok())
        .unwrap_or(false)
}

fn sqlite_column_exists(db: &Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = db.prepare(&format!(
        "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1 LIMIT 1"
    )) else {
        return false;
    };
    stmt.exists([column]).unwrap_or(false)
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn clamp_search_limit(limit: usize) -> usize {
    limit.clamp(1, SEARCH_RESULT_LIMIT)
}

fn resume_command(session_id: &str) -> Option<String> {
    if !session_id.is_empty()
        && session_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        Some(format!("claude --resume {session_id}"))
    } else {
        None
    }
}

fn resume_command_with_model(session_id: &str, model: &str) -> Option<String> {
    let base = resume_command(session_id)?;
    let model = model.trim();
    if model.is_empty()
        || !model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'))
    {
        return Some(base);
    }
    Some(format!("{base} --model {model}"))
}

/// Result of forking a Claude Code session onto a new model identity.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMigrateResult {
    pub session: SessionMeta,
    pub source_session_id: String,
    pub target_model: String,
    pub lines_copied: usize,
    pub lines_trimmed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_model: Option<String>,
    /// Thinking / redacted_thinking blocks removed (cross-provider signatures are invalid).
    pub thinking_blocks_stripped: usize,
    /// Assistant turns that only contained API/auth errors and were dropped.
    pub error_turns_removed: usize,
    /// Older history lines dropped to fit the portable context budget.
    pub history_lines_compacted: usize,
}

const MAX_MIGRATE_LINES: usize = 50_000;
/// Keep roughly one short successful upstream turn of headroom after system/tools (~16k).
const MIGRATE_HISTORY_CHAR_BUDGET: usize = 24_000;
/// Cap individual tool_result payloads so one Bash dump cannot blow the budget.
const MIGRATE_TOOL_RESULT_CHAR_CAP: usize = 4_000;

/// Fork a Claude Code JSONL session: new id, rewrite model fields, strip
/// non-portable thinking/signatures, drop error turns, compact history, and
/// trim incomplete tool rounds. Original file is left untouched.
pub fn migrate_claude_code_session(
    source_path: &str,
    target_model: &str,
) -> AppResult<SessionMigrateResult> {
    let target_model = target_model.trim();
    if target_model.is_empty() {
        return Err(AppError::Config(
            "目标模型不能为空，请先切换供应商或指定模型".into(),
        ));
    }
    let source = PathBuf::from(source_path);
    if !source.is_file() {
        return Err(AppError::Config(format!("会话文件不存在: {source_path}")));
    }
    let parent = source.parent().ok_or_else(|| {
        AppError::Config(format!("无法确定会话目录: {source_path}"))
    })?;

    let file = open_session_file(&source)?;
    let reader = BufReader::new(file);
    let mut raw_lines: Vec<String> = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| {
            AppError::Io(format!("读取会话失败: {error}"))
        })?;
        if raw_lines.len() >= MAX_MIGRATE_LINES {
            return Err(AppError::Config(format!(
                "会话过大（超过 {MAX_MIGRATE_LINES} 行），请改用摘要复制到新会话"
            )));
        }
        raw_lines.push(line);
    }

    let mut values: Vec<Value> = Vec::with_capacity(raw_lines.len());
    let mut previous_model = None;
    let mut source_session_id = None;
    for line in &raw_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(trimmed) else {
            // Keep non-JSON lines out of the rewrite path; skip them.
            continue;
        };
        if source_session_id.is_none() {
            source_session_id = value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if previous_model.is_none() {
            previous_model = value
                .pointer("/message/model")
                .and_then(Value::as_str)
                .filter(|model| *model != "<synthetic>")
                .map(str::to_string)
                .or_else(|| {
                    value
                        .get("model")
                        .and_then(Value::as_str)
                        .filter(|model| *model != "<synthetic>")
                        .map(str::to_string)
                });
        }
        values.push(std::mem::take(&mut value));
    }

    let source_session_id = source_session_id.or_else(|| {
        source
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
    });
    let Some(source_session_id) = source_session_id else {
        return Err(AppError::Config("无法解析源会话 ID".into()));
    };

    let sanitize = sanitize_migrated_session_values(&mut values);
    // Cross-provider hard-resume of Claude compact summaries + embedded file
    // attachments is rejected by OpenAI-compatible gateways (opaque upstream 502).
    // Prefer a portable one-turn seed when a continuation summary is present.
    let used_seed = maybe_replace_with_portable_seed(&mut values, &source_session_id);
    let history_lines_compacted = if used_seed {
        0
    } else {
        compact_migrated_history_smart(&mut values, MIGRATE_HISTORY_CHAR_BUDGET)
    };
    let orphan_results_dropped = if used_seed {
        0
    } else {
        drop_orphan_tool_results(&mut values)
    };
    let lines_trimmed = if used_seed {
        sanitize.lines_removed
    } else {
        truncate_incomplete_tool_rounds(&mut values) + orphan_results_dropped + sanitize.lines_removed
    };
    // Compaction / error-turn removal can leave dangling parents; repair once more.
    repair_parent_uuid_chain(&mut values);

    let new_session_id = uuid::Uuid::new_v4().to_string();
    for value in &mut values {
        rewrite_session_id(value, &new_session_id);
        rewrite_model_fields(value, target_model);
    }

    let dest = parent.join(format!("{new_session_id}.jsonl"));
    if dest.exists() {
        return Err(AppError::Config(format!(
            "目标会话文件已存在: {}",
            dest.display()
        )));
    }
    let tmp = parent.join(format!(".{new_session_id}.jsonl.tmp"));
    {
        let mut out = File::create(&tmp).map_err(|error| {
            AppError::Io(format!("创建临时会话文件失败: {error}"))
        })?;
        for value in &values {
            writeln!(out, "{}", serde_json::to_string(value).map_err(|error| {
                AppError::Config(format!("序列化会话行失败: {error}"))
            })?)
            .map_err(|error| AppError::Io(format!("写入会话失败: {error}")))?;
        }
        out.flush()
            .map_err(|error| AppError::Io(format!("刷新会话文件失败: {error}")))?;
    }
    fs::rename(&tmp, &dest).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        AppError::Io(format!("提交会话文件失败: {error}"))
    })?;

    let session = parse_claude_code_session(&dest)?.ok_or_else(|| {
        AppError::Config("迁移后无法解析新会话".into())
    })?;
    let mut session = session;
    session.resume_command = resume_command_with_model(&new_session_id, target_model);

    Ok(SessionMigrateResult {
        session,
        source_session_id,
        target_model: target_model.to_string(),
        lines_copied: values.len(),
        lines_trimmed,
        previous_model,
        thinking_blocks_stripped: sanitize.thinking_blocks_stripped,
        error_turns_removed: sanitize.error_turns_removed,
        history_lines_compacted,
    })
}

#[derive(Debug, Default)]
struct MigrateSanitizeStats {
    thinking_blocks_stripped: usize,
    error_turns_removed: usize,
    lines_removed: usize,
}

fn sanitize_migrated_session_values(values: &mut Vec<Value>) -> MigrateSanitizeStats {
    let mut stats = MigrateSanitizeStats::default();
    let mut removed_uuids: HashMap<String, Option<String>> = HashMap::new();
    let mut kept: Vec<Value> = Vec::with_capacity(values.len());

    for mut value in values.drain(..) {
        if is_error_or_synthetic_assistant(&value) {
            if let Some(uuid) = value.get("uuid").and_then(Value::as_str) {
                let parent = value
                    .get("parentUuid")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                removed_uuids.insert(uuid.to_string(), parent);
            }
            stats.error_turns_removed += 1;
            stats.lines_removed += 1;
            continue;
        }

        stats.thinking_blocks_stripped += strip_thinking_blocks(&mut value);
        truncate_tool_results_in_value(&mut value, MIGRATE_TOOL_RESULT_CHAR_CAP);

        if is_empty_message_turn(&value) {
            if let Some(uuid) = value.get("uuid").and_then(Value::as_str) {
                let parent = value
                    .get("parentUuid")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                removed_uuids.insert(uuid.to_string(), parent);
            }
            stats.lines_removed += 1;
            continue;
        }

        kept.push(value);
    }

    for value in &mut kept {
        reparent_value(value, &removed_uuids);
    }
    *values = kept;
    stats
}

fn is_error_or_synthetic_assistant(value: &Value) -> bool {
    let Some(message) = value.get("message") else {
        return false;
    };
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return false;
    }
    if message.get("model").and_then(Value::as_str) == Some("<synthetic>") {
        return true;
    }
    let text = message_text_preview(message);
    let lower = text.to_ascii_lowercase();
    lower.contains("api error:")
        || lower.contains("failed to authenticate")
        || text.trim() == "No response requested."
        || lower.contains("upstream request failed")
}

fn message_text_preview(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    item.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn strip_thinking_blocks(value: &mut Value) -> usize {
    let Some(content) = value
        .pointer_mut("/message/content")
        .and_then(Value::as_array_mut)
    else {
        return 0;
    };
    let before = content.len();
    content.retain(|item| {
        !matches!(
            item.get("type").and_then(Value::as_str),
            Some("thinking" | "redacted_thinking")
        )
    });
    before.saturating_sub(content.len())
}

fn truncate_tool_results_in_value(value: &mut Value, cap: usize) {
    let Some(content) = value
        .pointer_mut("/message/content")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for item in content.iter_mut() {
        if item.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        match item.get_mut("content") {
            Some(Value::String(text)) if text.len() > cap => {
                text.truncate(cap);
                text.push_str("\n…[truncated by AI-Switcher migrate]");
            }
            Some(Value::Array(blocks)) => {
                for block in blocks.iter_mut() {
                    if let Some(Value::String(text)) = block.get_mut("text") {
                        if text.len() > cap {
                            text.truncate(cap);
                            text.push_str("\n…[truncated by AI-Switcher migrate]");
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_empty_message_turn(value: &Value) -> bool {
    let Some(message) = value.get("message") else {
        return false;
    };
    if message.get("role").and_then(Value::as_str).is_none() {
        return false;
    }
    match message.get("content") {
        None => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Null) => true,
        _ => false,
    }
}

fn reparent_value(value: &mut Value, removed: &HashMap<String, Option<String>>) {
    let Some(parent) = value
        .get("parentUuid")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    let mut current = parent;
    let mut guard = 0;
    while let Some(mapped) = removed.get(&current) {
        guard += 1;
        if guard > 64 {
            break;
        }
        match mapped {
            Some(next) => current = next.clone(),
            None => {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("parentUuid".into(), Value::Null);
                }
                return;
            }
        }
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert("parentUuid".into(), Value::String(current));
    }
}

fn estimate_value_message_chars(value: &Value) -> usize {
    let Some(message) = value.get("message") else {
        return 0;
    };
    match message.get("content") {
        Some(Value::String(text)) => text.len(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item.get("type").and_then(Value::as_str) {
                Some("text") => item
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0),
                Some("tool_use") => {
                    item.get("name")
                        .and_then(Value::as_str)
                        .map(str::len)
                        .unwrap_or(0)
                        + item
                            .get("input")
                            .map(|input| input.to_string().len())
                            .unwrap_or(0)
                }
                Some("tool_result") => match item.get("content") {
                    Some(Value::String(text)) => text.len(),
                    Some(Value::Array(blocks)) => blocks
                        .iter()
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .map(str::len)
                        .sum(),
                    _ => 0,
                },
                _ => 0,
            })
            .sum(),
        _ => 0,
    }
}

/// Drop oldest message-bearing lines until remaining content fits `budget`.
/// Prefers Claude Code's own context-continuation summary when present, and
/// never cuts in the middle of a tool_use / tool_result round.
fn compact_migrated_history_smart(values: &mut Vec<Value>, budget: usize) -> usize {
    if let Some(summary_idx) = find_context_continuation_index(values) {
        // Claude Code already compacted once; keep from that portable summary.
        return drain_prefix(values, summary_idx);
    }
    compact_migrated_history(values, budget)
}

fn find_context_continuation_index(values: &[Value]) -> Option<usize> {
    values.iter().position(|value| {
        let Some(message) = value.get("message") else {
            return false;
        };
        let text = message_text_preview(message);
        text.contains("This session is being continued from a previous conversation")
    })
}

/// When Claude Code already emitted a context-continuation summary, hard-resuming
/// the surrounding JSONL (attachments / local commands / resume meta-instructions)
/// commonly yields opaque upstream 502 on OpenAI-compatible gateways. Replace the
/// whole transcript with one portable user seed turn.
fn maybe_replace_with_portable_seed(values: &mut Vec<Value>, source_session_id: &str) -> bool {
    let Some(summary_idx) = find_context_continuation_index(values) else {
        return false;
    };
    let Some(message) = values[summary_idx].get("message") else {
        return false;
    };
    let mut summary = message_text_preview(message);
    for marker in [
        "If you need specific details from before compaction",
        "Continue the conversation from where it left off without asking",
    ] {
        if let Some(idx) = summary.find(marker) {
            summary.truncate(idx);
            summary = summary.trim_end().to_string();
        }
    }
    // Claude Code treats the stock continuation phrase as a compact-resume
    // marker; keeping it in a migrated seed makes OpenAI-compatible gateways
    // return opaque upstream 502 while blank sessions still work.
    summary = summary
        .replace(
            "This session is being continued from a previous conversation that ran out of context.",
            "",
        )
        .replace(
            "The summary below covers the earlier portion of the conversation.",
            "",
        );
    if let Some(idx) = summary.find("Summary:") {
        summary = summary[idx..].to_string();
    }
    summary = summary.trim().to_string();
    const SEED_SUMMARY_CAP: usize = 3_500;
    if summary.chars().count() > SEED_SUMMARY_CAP {
        summary = summary.chars().take(SEED_SUMMARY_CAP).collect::<String>()
            + "\n…（摘要已截断以便跨供应商续聊）";
    }
    if summary.trim().is_empty() {
        return false;
    }

    let cwd = values
        .iter()
        .find_map(|value| value.get("cwd").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let timestamp = values[summary_idx]
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or("2026-01-01T00:00:00.000Z")
        .to_string();
    let seed = format!(
        "【迁移上下文｜普通用户消息】\n\
以下是从旧会话 `{source_session_id}` 整理的工作摘要（不是 Claude compact continuation，请按普通任务继续）：\n\n\
{summary}\n\n\
请从上次中断处继续推进任务；先确认当前缺口，再做最小必要改动。"
    );
    let user_uuid = uuid::Uuid::new_v4().to_string();
    let mut user = serde_json::json!({
        "type": "user",
        "uuid": user_uuid,
        "parentUuid": Value::Null,
        "isSidechain": false,
        "sessionId": source_session_id,
        "timestamp": timestamp,
        "userType": "external",
        "message": { "role": "user", "content": seed },
    });
    if !cwd.is_empty() {
        if let Some(obj) = user.as_object_mut() {
            obj.insert("cwd".into(), Value::String(cwd));
        }
    }
    *values = vec![
        user,
        serde_json::json!({
            "type": "last-prompt",
            "lastPrompt": "请从迁移摘要继续",
            "leafUuid": user_uuid,
            "sessionId": source_session_id,
        }),
        serde_json::json!({
            "type": "mode",
            "mode": "normal",
            "sessionId": source_session_id,
        }),
        serde_json::json!({
            "type": "permission-mode",
            "permissionMode": "default",
            "sessionId": source_session_id,
        }),
    ];
    true
}

fn compact_migrated_history(values: &mut Vec<Value>, budget: usize) -> usize {
    let total: usize = values.iter().map(estimate_value_message_chars).sum();
    if total <= budget {
        return 0;
    }

    let mut keep_from = values.len();
    let mut used = 0usize;
    for idx in (0..values.len()).rev() {
        let chars = estimate_value_message_chars(&values[idx]);
        if chars == 0 {
            continue;
        }
        if used + chars > budget && keep_from < values.len() {
            break;
        }
        used += chars;
        keep_from = idx;
    }

    // Prefer starting on a user turn so the API history opens cleanly.
    while keep_from < values.len() {
        let is_user = values[keep_from]
            .pointer("/message/role")
            .and_then(Value::as_str)
            == Some("user");
        if is_user || estimate_value_message_chars(&values[keep_from]) == 0 {
            break;
        }
        keep_from += 1;
    }

    // Do not cut inside a tool round: advance until the suffix has no orphan results.
    keep_from = align_keep_from_without_orphan_results(values, keep_from);

    if keep_from == 0 {
        return 0;
    }
    drain_prefix(values, keep_from)
}

fn align_keep_from_without_orphan_results(values: &[Value], mut keep_from: usize) -> usize {
    while keep_from < values.len() {
        if suffix_has_orphan_tool_result(&values[keep_from..]) {
            keep_from += 1;
            continue;
        }
        break;
    }
    keep_from
}

fn suffix_has_orphan_tool_result(values: &[Value]) -> bool {
    let mut open: HashSet<String> = HashSet::new();
    for value in values {
        // tool_use must be observed before matching tool_result in the suffix.
        for id in collect_tool_use_ids(value) {
            open.insert(id);
        }
        for id in collect_tool_result_ids(value) {
            if !open.contains(&id) {
                return true;
            }
            open.remove(&id);
        }
    }
    false
}

fn drain_prefix(values: &mut Vec<Value>, keep_from: usize) -> usize {
    if keep_from == 0 || keep_from >= values.len() {
        return 0;
    }
    let removed: HashMap<String, Option<String>> = values[..keep_from]
        .iter()
        .filter_map(|value| {
            let uuid = value.get("uuid").and_then(Value::as_str)?.to_string();
            let parent = value
                .get("parentUuid")
                .and_then(Value::as_str)
                .map(str::to_string);
            Some((uuid, parent))
        })
        .collect();
    let compacted = keep_from;
    values.drain(..keep_from);
    for value in values.iter_mut() {
        reparent_value(value, &removed);
    }
    if let Some(first) = values
        .iter_mut()
        .find(|value| value.get("message").is_some())
    {
        if let Some(obj) = first.as_object_mut() {
            obj.insert("parentUuid".into(), Value::Null);
        }
    }
    compacted
}

/// Remove tool_result blocks whose tool_use was dropped by compaction/sanitize.
fn drop_orphan_tool_results(values: &mut Vec<Value>) -> usize {
    let known_uses: HashSet<String> = values
        .iter()
        .flat_map(collect_tool_use_ids)
        .collect();
    let mut removed_uuids: HashMap<String, Option<String>> = HashMap::new();
    let mut kept: Vec<Value> = Vec::with_capacity(values.len());
    let mut dropped_lines = 0usize;

    for mut value in values.drain(..) {
        let mut dropped_blocks = 0usize;
        if let Some(content) = value
            .pointer_mut("/message/content")
            .and_then(Value::as_array_mut)
        {
            let before = content.len();
            content.retain(|item| {
                if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                    return true;
                }
                item.get("tool_use_id")
                    .or_else(|| item.get("toolUseId"))
                    .and_then(Value::as_str)
                    .is_some_and(|id| known_uses.contains(id))
            });
            dropped_blocks = before.saturating_sub(content.len());
        }

        if dropped_blocks > 0 && is_empty_message_turn(&value) {
            if let Some(uuid) = value.get("uuid").and_then(Value::as_str) {
                let parent = value
                    .get("parentUuid")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                removed_uuids.insert(uuid.to_string(), parent);
            }
            dropped_lines += 1;
            continue;
        }
        kept.push(value);
    }

    for value in &mut kept {
        reparent_value(value, &removed_uuids);
    }
    *values = kept;
    dropped_lines
}

fn repair_parent_uuid_chain(values: &mut Vec<Value>) {
    let existing: HashSet<String> = values
        .iter()
        .filter_map(|value| value.get("uuid").and_then(Value::as_str).map(str::to_string))
        .collect();
    for value in values.iter_mut() {
        let Some(parent) = value.get("parentUuid").and_then(Value::as_str) else {
            continue;
        };
        if parent.is_empty() || existing.contains(parent) {
            continue;
        }
        if let Some(obj) = value.as_object_mut() {
            obj.insert("parentUuid".into(), Value::Null);
        }
    }
}

fn rewrite_session_id(value: &mut Value, new_id: &str) {
    if let Some(obj) = value.as_object_mut() {
        if obj.contains_key("sessionId") {
            obj.insert("sessionId".into(), Value::String(new_id.to_string()));
        }
    }
}

fn rewrite_model_fields(value: &mut Value, target_model: &str) {
    if let Some(obj) = value.as_object_mut() {
        if let Some(message) = obj.get_mut("message").and_then(Value::as_object_mut) {
            if message.contains_key("model")
                || message.get("role").and_then(Value::as_str) == Some("assistant")
            {
                message.insert("model".into(), Value::String(target_model.to_string()));
            }
        }
        if obj.contains_key("model")
            && obj.get("type").and_then(Value::as_str) != Some("file-history-snapshot")
        {
            // Only rewrite top-level model when it looks like an assistant turn marker.
            if obj.get("type").and_then(Value::as_str) == Some("assistant")
                || obj
                    .get("message")
                    .and_then(|m| m.get("role"))
                    .and_then(Value::as_str)
                    == Some("assistant")
            {
                obj.insert("model".into(), Value::String(target_model.to_string()));
            }
        }
    }
}

fn collect_tool_use_ids(value: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    let content = value
        .pointer("/message/content")
        .or_else(|| value.get("content"));
    match content {
        Some(Value::Array(items)) => {
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    ids
}

fn collect_tool_result_ids(value: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    let content = value
        .pointer("/message/content")
        .or_else(|| value.get("content"));
    match content {
        Some(Value::Array(items)) => {
            for item in items {
                let is_result = item.get("type").and_then(Value::as_str) == Some("tool_result");
                if is_result {
                    if let Some(id) = item
                        .get("tool_use_id")
                        .or_else(|| item.get("toolUseId"))
                        .and_then(Value::as_str)
                    {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    ids
}

/// Drop trailing lines that leave unpaired `tool_use` blocks (common after 502/abort).
/// Returns number of lines removed.
fn truncate_incomplete_tool_rounds(values: &mut Vec<Value>) -> usize {
    if values.is_empty() {
        return 0;
    }
    let mut open: HashSet<String> = HashSet::new();
    let mut cut: Option<usize> = None;
    for (idx, value) in values.iter().enumerate() {
        for id in collect_tool_use_ids(value) {
            open.insert(id);
        }
        for id in collect_tool_result_ids(value) {
            open.remove(&id);
        }
        if open.is_empty() {
            cut = None;
        } else if cut.is_none() {
            cut = Some(idx);
        }
    }
    let Some(cut_idx) = cut else {
        return 0;
    };
    let trimmed = values.len() - cut_idx;
    values.truncate(cut_idx);
    trimmed
}

// ---- OpenCode sessions (SQLite opencode.db + legacy JSON storage) -----------
//
// 参考 cc-switch `session_manager/providers/opencode.rs`：新版 OpenCode 会话
// 存于 `~/.local/share/opencode/opencode.db`（session/message/part 三表），
// 旧版为 `storage/session|message|part/**/*.json`。SQLite 优先，JSON 补充去重。
// SQLite 会话的 source_path 是合成引用 `sqlite:<db路径>:<session_id>`。

fn opencode_storage_dir() -> PathBuf {
    config::get_opencode_data_dir().join("storage")
}

fn scan_opencode_sessions() -> (Vec<SessionMeta>, SessionProviderStatus) {
    let mut sessions = scan_opencode_sessions_sqlite();
    let json_sessions = scan_opencode_sessions_json();
    if !json_sessions.is_empty() {
        let known: HashSet<String> = sessions.iter().map(|s| s.session_id.clone()).collect();
        for meta in json_sessions {
            if !known.contains(&meta.session_id) {
                sessions.push(meta);
            }
        }
    }
    let data_dir = config::get_opencode_data_dir();
    let exists = data_dir.exists();
    let status = if exists {
        SessionProviderStatus {
            provider: SessionProvider::OpenCode,
            status: "available".to_string(),
            detail: format!("OpenCode 本地会话可用（{} 个）", sessions.len()),
            root_path: Some(data_dir.display().to_string()),
        }
    } else {
        SessionProviderStatus {
            provider: SessionProvider::OpenCode,
            status: "not_found".to_string(),
            detail: "未发现 OpenCode 本地数据目录".to_string(),
            root_path: Some(data_dir.display().to_string()),
        }
    };
    (sessions, status)
}

fn scan_opencode_sessions_sqlite() -> Vec<SessionMeta> {
    let db_path = config::get_opencode_db_path();
    if !db_path.exists() {
        return Vec::new();
    }
    let conn = match Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(error) => {
            log::warn!("无法打开 OpenCode 数据库 {}: {error}", db_path.display());
            return Vec::new();
        }
    };
    let mut stmt = match conn.prepare(
        "SELECT id, title, directory, time_created, time_updated FROM session ORDER BY time_updated DESC",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            log::warn!("OpenCode 数据库 session 表查询失败: {error}");
            return Vec::new();
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(error) => {
            log::warn!("OpenCode 数据库 session 读取失败: {error}");
            return Vec::new();
        }
    };
    let db_display = db_path.display().to_string();
    let mut sessions = Vec::new();
    for row in rows.flatten() {
        let (session_id, title, directory, created, updated) = row;
        let display_title = if title.is_empty() {
            opencode_path_basename(&directory).map(str::to_string)
        } else {
            Some(title)
        };
        sessions.push(SessionMeta {
            provider: SessionProvider::OpenCode,
            session_id: session_id.clone(),
            title: display_title.clone(),
            summary: display_title,
            project_dir: (!directory.is_empty()).then_some(directory),
            created_at: Some(created),
            last_active_at: Some(updated),
            source_path: format!("sqlite:{db_display}:{session_id}"),
            resume_command: Some(format!("opencode -s {session_id}")),
            pinned: false,
        });
    }
    sessions
}

fn scan_opencode_sessions_json() -> Vec<SessionMeta> {
    let session_dir = opencode_storage_dir().join("session");
    if !session_dir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    collect_opencode_json_files(&session_dir, &mut files, 0);
    files
        .iter()
        .filter_map(|path| parse_opencode_session_json(path))
        .collect()
}

/// storage/session/<project>/<session>.json 为两层结构，限制深度防止符号链接扩散。
fn collect_opencode_json_files(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        // 不跟随目录符号链接，避免扩大读取边界。
        let Ok(meta) = fs::symlink_metadata(entry.path()) else { continue };
        let path = entry.path();
        if meta.is_dir() {
            collect_opencode_json_files(&path, out, depth + 1);
        } else if meta.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("json")
        {
            out.push(path);
        }
    }
}

fn parse_opencode_session_json(path: &Path) -> Option<SessionMeta> {
    let data = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;
    let session_id = value.get("id").and_then(Value::as_str)?.to_string();
    let directory = value
        .get("directory")
        .and_then(Value::as_str)
        .map(str::to_string);
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| directory.as_deref().and_then(opencode_path_basename).map(str::to_string));
    let created_at = value
        .pointer("/time/created")
        .and_then(parse_opencode_timestamp_ms);
    let updated_at = value
        .pointer("/time/updated")
        .and_then(parse_opencode_timestamp_ms);

    Some(SessionMeta {
        provider: SessionProvider::OpenCode,
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir: directory,
        created_at,
        last_active_at: updated_at.or(created_at),
        // JSON 存储的消息在 storage/message/<sessionID>/ 目录下。
        source_path: opencode_storage_dir()
            .join("message")
            .join(&session_id)
            .display()
            .to_string(),
        resume_command: Some(format!("opencode -s {session_id}")),
        pinned: false,
    })
}

fn parse_opencode_timestamp_ms(value: &Value) -> Option<i64> {
    if let Some(num) = value.as_i64() {
        // OpenCode 存毫秒；兼容秒级时间戳。
        return Some(if num < 10_000_000_000 { num * 1000 } else { num });
    }
    value
        .as_str()
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|dt| dt.timestamp_millis())
}

fn opencode_path_basename(path: &str) -> Option<&str> {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
}

/// 解析 `sqlite:<db路径>:<session_id>`。session_id 在最后一段（Windows 路径含盘符冒号）。
fn parse_opencode_sqlite_source(source: &str) -> Option<(PathBuf, String)> {
    let rest = source.strip_prefix("sqlite:")?;
    let (db_path, session_id) = rest.rsplit_once(':')?;
    if db_path.is_empty() || session_id.is_empty() {
        return None;
    }
    Some((PathBuf::from(db_path), session_id.to_string()))
}

fn load_opencode_messages(source_path: &str) -> AppResult<Vec<SessionMessage>> {
    if source_path.starts_with("sqlite:") {
        let (db_path, session_id) = parse_opencode_sqlite_source(source_path)
            .ok_or_else(|| AppError::Path(format!("OpenCode 会话引用无效: {source_path}")))?;
        return load_opencode_messages_sqlite(&db_path, &session_id);
    }
    // JSON 存储：source_path = storage/message/<sessionID>/ 目录。
    let dir = PathBuf::from(source_path);
    let storage = opencode_storage_dir();
    let dir_key = normalize_path_key(&dir);
    let root_key = normalize_path_key(&storage.join("message"));
    if !dir_key.starts_with(&root_key) {
        return Err(AppError::Path(format!(
            "OpenCode 会话目录不在允许的目录内: {}",
            dir.display()
        )));
    }
    load_opencode_messages_json(&storage, &dir)
}

fn load_opencode_messages_sqlite(db_path: &Path, session_id: &str) -> AppResult<Vec<SessionMessage>> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| AppError::Config(format!("无法打开 OpenCode 数据库: {error}")))?;

    let mut msg_stmt = conn
        .prepare("SELECT id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created ASC")
        .map_err(|error| AppError::Config(format!("OpenCode 消息查询失败: {error}")))?;
    let msg_rows = msg_stmt
        .query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| AppError::Config(format!("OpenCode 消息读取失败: {error}")))?;

    let mut part_stmt = conn
        .prepare("SELECT message_id, data FROM part WHERE session_id = ?1 ORDER BY time_created ASC")
        .map_err(|error| AppError::Config(format!("OpenCode 消息块查询失败: {error}")))?;
    let part_rows = part_stmt
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| AppError::Config(format!("OpenCode 消息块读取失败: {error}")))?;

    let mut parts_map: HashMap<String, Vec<String>> = HashMap::new();
    for row in part_rows.flatten() {
        let (message_id, data) = row;
        parts_map.entry(message_id).or_default().push(data);
    }

    let mut messages = Vec::new();
    for row in msg_rows.flatten() {
        let (msg_id, ts, data) = row;
        let value: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let role = value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let mut texts = Vec::new();
        if let Some(parts) = parts_map.get(&msg_id) {
            for part_data in parts {
                if let Ok(part_value) = serde_json::from_str::<Value>(part_data) {
                    if let Some(text) = extract_opencode_part_text(&part_value) {
                        texts.push(text);
                    }
                }
            }
        }
        let content = texts.join("\n\n");
        if content.trim().is_empty() {
            continue;
        }
        messages.push(SessionMessage {
            role,
            content,
            timestamp: (ts > 0).then_some(ts),
        });
    }
    Ok(messages)
}

fn load_opencode_messages_json(storage: &Path, msg_dir: &Path) -> AppResult<Vec<SessionMessage>> {
    if !msg_dir.is_dir() {
        return Err(AppError::Path(format!(
            "找不到 OpenCode 会话消息目录: {}",
            msg_dir.display()
        )));
    }
    let mut files = Vec::new();
    collect_opencode_json_files(msg_dir, &mut files, 0);

    let mut entries: Vec<(i64, String, String)> = Vec::new();
    for path in files {
        let value: Value = fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or(Value::Null);
        let Some(msg_id) = value.get("id").and_then(Value::as_str) else { continue };
        let role = value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let created = value
            .pointer("/time/created")
            .and_then(parse_opencode_timestamp_ms)
            .unwrap_or(0);
        let text = collect_opencode_parts_text(&storage.join("part").join(msg_id));
        if text.trim().is_empty() {
            continue;
        }
        entries.push((created, role, text));
    }
    entries.sort_by_key(|(ts, _, _)| *ts);
    Ok(entries
        .into_iter()
        .map(|(ts, role, content)| SessionMessage {
            role,
            content,
            timestamp: (ts > 0).then_some(ts),
        })
        .collect())
}

fn extract_opencode_part_text(part: &Value) -> Option<String> {
    match part.get("type").and_then(Value::as_str) {
        Some("text") => part
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string),
        Some("tool") => {
            let tool = part.get("tool").and_then(Value::as_str).unwrap_or("unknown");
            Some(format!("[Tool: {tool}]"))
        }
        _ => None,
    }
}

fn collect_opencode_parts_text(part_dir: &Path) -> String {
    if !part_dir.is_dir() {
        return String::new();
    }
    let mut files = Vec::new();
    collect_opencode_json_files(part_dir, &mut files, 0);
    let mut texts = Vec::new();
    for path in files {
        let value: Value = match fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
        {
            Some(value) => value,
            None => continue,
        };
        if let Some(text) = extract_opencode_part_text(&value) {
            texts.push(text);
        }
    }
    texts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_session(path: &Path) {
        let mut file = File::create(path).unwrap();
        writeln!(
            file,
            r#"{{"sessionId":"session-1","cwd":"C:\\work","timestamp":"2026-03-01T12:00:00Z","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(file, "not-json").unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-03-01T12:00:01Z","message":{{"role":"assistant","content":[{{"type":"text","text":"world"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-03-01T12:00:02Z","message":{{"role":"user","content":[{{"type":"tool_result","content":"done"}}]}}}}"#
        )
        .unwrap();
    }

    #[test]
    fn parses_metadata_and_tolerates_broken_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-1.jsonl");
        write_session(&path);
        let session = parse_claude_code_session(&path).unwrap().unwrap();
        assert_eq!(session.session_id, "session-1");
        assert_eq!(session.project_dir.as_deref(), Some("C:\\work"));
        assert_eq!(session.summary.as_deref(), Some("hello"));
    }

    #[test]
    fn loads_normalized_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-1.jsonl");
        write_session(&path);
        let messages = load_claude_code_messages(&path).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].content, "world");
        assert_eq!(messages[2].role, "tool");
    }

    #[test]
    fn rejects_paths_outside_the_session_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("session.jsonl");
        File::create(&source).unwrap();
        let error = validate_session_path_in_root(root.path(), &source).unwrap_err();
        assert!(error.to_string().contains("不在允许的目录内"));
    }

    #[test]
    fn ignores_agent_session_files() {
        let dir = tempfile::tempdir().unwrap();
        File::create(dir.path().join("agent-child.jsonl")).unwrap();
        File::create(dir.path().join("parent.jsonl")).unwrap();
        let mut files = Vec::new();
        collect_jsonl_files(dir.path(), &mut files).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "parent.jsonl");
    }

    #[test]
    fn empty_session_keeps_basic_file_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty-session.jsonl");
        File::create(&path).unwrap();
        let session = parse_claude_code_session(&path).unwrap().unwrap();
        assert_eq!(session.session_id, "empty-session");
        assert!(session.summary.is_none());
        assert!(load_claude_code_messages(&path).unwrap().is_empty());
    }

    #[test]
    fn full_text_search_limit_is_bounded() {
        assert_eq!(clamp_search_limit(0), 1);
        assert_eq!(clamp_search_limit(20), 20);
        assert_eq!(clamp_search_limit(usize::MAX), 200);
    }

    #[test]
    fn resume_command_rejects_shell_metacharacters() {
        assert_eq!(
            resume_command("safe-session_1").as_deref(),
            Some("claude --resume safe-session_1")
        );
        assert!(resume_command("unsafe & whoami").is_none());
    }

    #[test]
    fn old_archives_default_to_claude_code_and_provider_mismatch_is_rejected() {
        let manifest: SessionArchiveManifest = serde_json::from_value(serde_json::json!({
            "version": 1,
            "sessionId": "session-1",
            "relativePath": "project/session-1.jsonl",
            "createdAt": 1,
            "contentSha256": "abc"
        })).unwrap();
        assert_eq!(manifest.provider, SessionProvider::ClaudeCode);
        assert!(validate_manifest_provider(SessionProvider::ClaudeCode, &manifest).is_ok());
        assert!(validate_manifest_provider(SessionProvider::Codex, &manifest).is_err());
    }

    #[test]
    fn collect_codex_finds_nested_rollout_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CODEX_HOME", dir.path());
        let nested = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("08")
            .join("05");
        fs::create_dir_all(&nested).unwrap();
        let rollout = nested.join(
            "rollout-2026-08-05T12-00-00-019f8d32-4e9b-7551-acde-45e4c9a58e0b.jsonl",
        );
        File::create(&rollout).unwrap();
        File::create(nested.join("agent-child.jsonl")).unwrap();

        let result = scan_sessions(Some(SessionProvider::Codex), Some(0), Some(10)).unwrap();
        std::env::remove_var("CODEX_HOME");

        assert_eq!(result.total, 1, "providers={:?}", result.providers);
        assert_eq!(result.sessions[0].provider, SessionProvider::Codex);
        assert!(result.sessions[0]
            .source_path
            .replace('\\', "/")
            .ends_with("rollout-2026-08-05T12-00-00-019f8d32-4e9b-7551-acde-45e4c9a58e0b.jsonl"));
    }

    #[test]
    fn live_home_codex_scan_finds_sessions_when_enabled() {
        if std::env::var_os("AI_SWITCHER_LIVE_CODEX_SCAN").is_none() {
            return;
        }
        // Use the real profile CODEX_HOME (do not override).
        let result = scan_sessions(Some(SessionProvider::Codex), Some(0), Some(5)).unwrap();
        eprintln!(
            "live Codex scan total={} detail={:?}",
            result.total,
            result.providers.first().map(|p| (&p.status, &p.detail, &p.root_path))
        );
        assert!(
            result.total > 0,
            "expected real ~/.codex/sessions to be non-empty; providers={:?}",
            result.providers
        );
    }

    #[test]
    fn scan_result_pagination_slice_matches_offset_limit() {
        let sessions: Vec<_> = (0..5)
            .map(|index| SessionMeta {
                provider: SessionProvider::ClaudeCode,
                session_id: format!("s-{index}"),
                title: None,
                summary: None,
                project_dir: None,
                created_at: Some(index as i64),
                last_active_at: Some(index as i64),
                source_path: format!("/tmp/s-{index}.jsonl"),
                resume_command: None,
                pinned: false,
            })
            .collect();
        let total = sessions.len();
        let offset = 2usize;
        let limit = 2usize;
        let page: Vec<_> = sessions.into_iter().skip(offset).take(limit).collect();
        assert_eq!(total, 5);
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].session_id, "s-2");
        assert_eq!(page[1].session_id, "s-3");
    }

    #[test]
    fn codex_thread_index_enriches_name_pin_and_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("state_5.sqlite");
        let rollout = dir
            .path()
            .join("rollout-demo-019f8d32-4e9b-7551-acde-45e4c9a58e0b.jsonl");
        File::create(&rollout).unwrap();

        let db = Connection::open(&db_path).unwrap();
        db.execute_batch(
            "CREATE TABLE threads (
                id TEXT,
                rollout_path TEXT,
                title TEXT,
                name TEXT,
                preview TEXT,
                first_user_message TEXT,
                cwd TEXT,
                is_pinned INTEGER,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            );
            INSERT INTO threads VALUES (
                '019f8d32-4e9b-7551-acde-45e4c9a58e0b',
                NULL,
                'auto title',
                'Named thread',
                NULL,
                'hello',
                'C:\\work\\demo',
                1,
                1000,
                2000
            );",
        )
        .unwrap();
        drop(db);

        let mut index = CodexThreadIndex::default();
        load_codex_thread_index_from_db(&db_path, &mut index).unwrap();

        let mut session = session_meta_from_path(SessionProvider::Codex, &rollout, 9).unwrap();
        apply_codex_thread_meta(&mut session, &rollout, &index);
        assert!(session.pinned);
        assert_eq!(session.title.as_deref(), Some("Named thread"));
        assert_eq!(session.project_dir.as_deref(), Some("C:\\work\\demo"));
        assert_eq!(session.session_id, "019f8d32-4e9b-7551-acde-45e4c9a58e0b");
        assert_eq!(
            session.resume_command.as_deref(),
            Some("codex resume 019f8d32-4e9b-7551-acde-45e4c9a58e0b")
        );
    }

    #[test]
    fn normalize_path_key_strips_windows_extended_prefix() {
        let plain = normalize_path_key(Path::new(
            r"C:\Users\admin\.codex\sessions\2026\08\03\rollout.jsonl",
        ));
        let extended = normalize_path_key(Path::new(
            r"\\?\C:\Users\admin\.codex\sessions\2026\08\03\rollout.jsonl",
        ));
        assert_eq!(plain, extended);
    }

    #[test]
    fn loads_codex_response_item_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-demo.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-08-03T11:59:12Z","type":"session_meta","payload":{{"id":"abc"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-08-03T11:59:13Z","type":"response_item","payload":{{"type":"message","role":"developer","content":[{{"type":"input_text","text":"skip me"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-08-03T11:59:14Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"hello codex"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-08-03T11:59:15Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"hi there"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-08-03T11:59:16Z","type":"event_msg","payload":{{"type":"agent_message","message":"hi there"}}}}"#
        )
        .unwrap();
        let messages = load_codex_messages(&path).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello codex");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "hi there");
    }

    #[test]
    fn migrate_claude_code_forks_rewrites_model_and_trims_orphan_tools() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old-session.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","sessionId":"old-session","timestamp":"2026-08-01T10:00:00Z","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","sessionId":"old-session","timestamp":"2026-08-01T10:00:01Z","message":{{"role":"assistant","model":"old-model","content":[{{"type":"text","text":"hi"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","sessionId":"old-session","timestamp":"2026-08-01T10:00:02Z","message":{{"role":"assistant","model":"old-model","content":[{{"type":"tool_use","id":"tool_1","name":"Bash","input":{{"command":"ls"}}}}]}}}}"#
        )
        .unwrap();
        // Orphan tool_use (no tool_result) — should be trimmed.
        drop(file);

        let result = migrate_claude_code_session(path.to_str().unwrap(), "new-model").unwrap();
        assert_eq!(result.source_session_id, "old-session");
        assert_eq!(result.target_model, "new-model");
        assert_eq!(result.previous_model.as_deref(), Some("old-model"));
        assert_eq!(result.lines_trimmed, 1);
        assert_eq!(result.lines_copied, 2);
        assert_eq!(result.thinking_blocks_stripped, 0);
        assert_ne!(result.session.session_id, "old-session");
        assert!(result
            .session
            .resume_command
            .as_deref()
            .unwrap_or("")
            .contains("--model new-model"));

        let migrated = fs::read_to_string(&result.session.source_path).unwrap();
        assert!(migrated.contains("new-model"));
        assert!(!migrated.contains("old-model"));
        assert!(!migrated.contains("tool_1"));
        assert!(migrated.contains(&result.session.session_id));
        assert!(!migrated.contains("old-session"));
        // Original preserved.
        let original = fs::read_to_string(&path).unwrap();
        assert!(original.contains("old-session"));
        assert!(original.contains("tool_1"));
    }

    #[test]
    fn migrate_strips_thinking_and_error_turns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("think-session.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"think-session","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"think-session","message":{{"role":"assistant","model":"k3","content":[{{"type":"thinking","thinking":"secret","signature":"sig"}},{{"type":"text","text":"hi"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","uuid":"a2","parentUuid":"a1","sessionId":"think-session","message":{{"role":"assistant","model":"k3","content":[{{"type":"text","text":"API Error: 502 Upstream request failed"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","uuid":"u2","parentUuid":"a2","sessionId":"think-session","message":{{"role":"user","content":"retry"}}}}"#
        )
        .unwrap();
        drop(file);

        let result = migrate_claude_code_session(path.to_str().unwrap(), "gpt-test").unwrap();
        assert!(result.thinking_blocks_stripped >= 1);
        assert!(result.error_turns_removed >= 1);
        let migrated = fs::read_to_string(&result.session.source_path).unwrap();
        assert!(!migrated.contains("thinking"));
        assert!(!migrated.contains("signature"));
        assert!(!migrated.contains("API Error: 502"));
        assert!(migrated.contains("retry"));
        assert!(migrated.contains("gpt-test"));
    }

    #[test]
    fn migrate_drops_orphan_tool_result_after_compact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orphan.jsonl");
        let mut file = File::create(&path).unwrap();
        // A complete early turn (will be compacted away once budget is tiny via many filler chars).
        writeln!(
            file,
            r#"{{"type":"user","uuid":"u0","parentUuid":null,"sessionId":"orphan","message":{{"role":"user","content":"{}"}}}}"#,
            "x".repeat(30_000)
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","uuid":"a0","parentUuid":"u0","sessionId":"orphan","message":{{"role":"assistant","model":"k3","content":[{{"type":"tool_use","id":"tool_keep","name":"Bash","input":{{"command":"echo"}}}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","uuid":"u1","parentUuid":"a0","sessionId":"orphan","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"tool_keep","content":"ok"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","uuid":"u2","parentUuid":"u1","sessionId":"orphan","message":{{"role":"user","content":"This session is being continued from a previous conversation that ran out of context. Summary: fiber debug."}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","uuid":"u3","parentUuid":"u2","sessionId":"orphan","message":{{"role":"user","content":"continue"}}}}"#
        )
        .unwrap();
        drop(file);

        let result = migrate_claude_code_session(path.to_str().unwrap(), "gpt-test").unwrap();
        let migrated = fs::read_to_string(&result.session.source_path).unwrap();
        assert!(migrated.contains("This session is being continued"));
        assert!(migrated.contains("continue"));
        assert!(!migrated.contains("tool_keep"));
        assert!(!migrated.contains("tool_result"));
        // Continuation summaries become a one-turn portable seed without the
        // Claude compact-resume marker phrase.
        assert!(migrated.contains("迁移上下文"));
        assert!(!migrated.contains("This session is being continued from a previous conversation"));
        assert!(!migrated.contains("Continue the conversation from where it left off without asking"));
    }

    #[test]
    fn truncate_keeps_complete_tool_rounds() {
        let mut values = vec![
            serde_json::json!({
                "type":"assistant",
                "message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"X","input":{}}]}
            }),
            serde_json::json!({
                "type":"user",
                "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}
            }),
        ];
        assert_eq!(truncate_incomplete_tool_rounds(&mut values), 0);
        assert_eq!(values.len(), 2);
    }
}
