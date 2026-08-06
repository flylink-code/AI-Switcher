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

pub fn scan_sessions(
    provider: Option<SessionProvider>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> AppResult<SessionScanResult> {
    let mut indexed: Vec<(i64, PathBuf, SessionProvider)> = Vec::new();
    let mut providers = Vec::new();
    let mut truncated = false;
    let mut timed_out = false;

    if provider.is_none() || provider == Some(SessionProvider::ClaudeCode) {
        let (paths, status, was_truncated, walk_timed_out) = collect_claude_code_session_paths()?;
        truncated |= was_truncated;
        timed_out |= walk_timed_out;
        for (path, mtime) in paths {
            indexed.push((mtime, path, SessionProvider::ClaudeCode));
        }
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::Codex) {
        let (paths, status, was_truncated, walk_timed_out) = collect_codex_session_paths()?;
        truncated |= was_truncated;
        timed_out |= walk_timed_out;
        for (path, mtime) in paths {
            indexed.push((mtime, path, SessionProvider::Codex));
        }
        providers.push(status);
    }

    let codex_index = if indexed
        .iter()
        .any(|(_, _, session_provider)| *session_provider == SessionProvider::Codex)
    {
        load_codex_thread_index()
    } else {
        CodexThreadIndex::default()
    };

    indexed.sort_by(|left, right| {
        let left_pinned = left.2 == SessionProvider::Codex
            && codex_index.lookup(&left.1).is_some_and(|meta| meta.pinned);
        let right_pinned = right.2 == SessionProvider::Codex
            && codex_index.lookup(&right.1).is_some_and(|meta| meta.pinned);
        right_pinned
            .cmp(&left_pinned)
            .then_with(|| right.0.cmp(&left.0))
            .then_with(|| left.1.cmp(&right.1))
    });

    let total = indexed.len();
    let offset = offset.unwrap_or(0).min(total);
    let limit = limit.filter(|value| *value > 0);
    let page: Vec<(i64, PathBuf, SessionProvider)> = match limit {
        Some(limit) => indexed.into_iter().skip(offset).take(limit).collect(),
        None if offset > 0 => indexed.into_iter().skip(offset).collect(),
        None => indexed,
    };

    // List view must NEVER open session files. Opening cloud placeholders /
    // antivirus-locked jsonl on Windows can stall the kernel and freeze the OS.
    // Codex names / pins come from SQLite thread index instead.
    let sessions = page
        .into_iter()
        .filter_map(|(mtime, path, session_provider)| {
            let mut session = session_meta_from_path(session_provider, &path, mtime)?;
            if session_provider == SessionProvider::Codex {
                apply_codex_thread_meta(&mut session, &path, &codex_index);
            }
            Some(session)
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

fn session_root(provider: SessionProvider) -> AppResult<PathBuf> {
    match provider {
        SessionProvider::ClaudeCode => Ok(claude_code_session_root()),
        SessionProvider::Codex => Ok(codex_session_root()),
    }
}

fn parse_session(provider: SessionProvider, path: &Path) -> AppResult<Option<SessionMeta>> {
    match provider {
        SessionProvider::ClaudeCode => parse_claude_code_session(path),
        SessionProvider::Codex => parse_codex_session(path),
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
    let target = match provider { SessionProvider::Codex => "codex", _ => "claude-code" };
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
            return Ok((
                Vec::new(),
                SessionProviderStatus {
                    provider: SessionProvider::Codex,
                    status: "not_found".to_string(),
                    detail: "未发现 Codex 本地会话目录".to_string(),
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
        SessionProvider::Codex => None,
    };
    let resume = match provider {
        SessionProvider::ClaudeCode => resume_command(&session_id),
        SessionProvider::Codex => Some(format!("codex resume {session_id}")),
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
}
