//! Local session discovery and archive operations for Claude Code and Codex.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

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

pub fn scan_sessions(provider: Option<SessionProvider>) -> AppResult<SessionScanResult> {
    let mut sessions = Vec::new();
    let mut providers = Vec::new();

    if provider.is_none() || provider == Some(SessionProvider::ClaudeCode) {
        let (mut code_sessions, status) = scan_claude_code_sessions()?;
        sessions.append(&mut code_sessions);
        providers.push(status);
    }
    if provider.is_none() || provider == Some(SessionProvider::Codex) {
        let (mut codex_sessions, status) = scan_codex_sessions()?;
        sessions.append(&mut codex_sessions);
        providers.push(status);
    }

    sessions.sort_by(|left, right| {
        let left_time = left.last_active_at.or(left.created_at).unwrap_or(0);
        let right_time = right.last_active_at.or(right.created_at).unwrap_or(0);
        right_time.cmp(&left_time)
    });

    Ok(SessionScanResult {
        sessions,
        providers,
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

    let mut result = scan_sessions(provider)?;
    let limit = clamp_search_limit(limit);
    result.sessions.retain(|session| {
        if session_metadata_contains(session, &query) {
            return true;
        }
        file_contains(session.provider, &session.source_path, &query).unwrap_or(false)
    });
    result.sessions.truncate(limit);
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
            load_claude_code_messages(&source)
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

fn scan_claude_code_sessions() -> AppResult<(Vec<SessionMeta>, SessionProviderStatus)> {
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
        ));
    }

    let mut paths = Vec::new();
    collect_jsonl_files(&root, &mut paths)?;
    let mut sessions = Vec::new();
    for path in paths {
        let path = match validate_session_path_in_root(&root, &path) {
            Ok(value) => value,
            Err(error) => {
                log::warn!("跳过会话根目录外的文件 {}: {error}", path.display());
                continue;
            }
        };
        match parse_claude_code_session(&path) {
            Ok(Some(session)) => sessions.push(session),
            Ok(None) => {}
            Err(error) => log::warn!("跳过无法解析的 Claude Code 会话 {}: {error}", path.display()),
        }
    }

    Ok((
        sessions,
        SessionProviderStatus {
            provider: SessionProvider::ClaudeCode,
            status: "available".to_string(),
            detail: "Claude Code 本地会话可用".to_string(),
            root_path: Some(root.display().to_string()),
        },
    ))
}

fn scan_codex_sessions() -> AppResult<(Vec<SessionMeta>, SessionProviderStatus)> {
    let root = codex_session_root();
    if !root.is_dir() {
        return Ok((Vec::new(), SessionProviderStatus {
            provider: SessionProvider::Codex,
            status: "not_found".to_string(),
            detail: "未发现 Codex 本地会话目录".to_string(),
            root_path: Some(root.display().to_string()),
        }));
    }
    let mut paths = Vec::new();
    collect_jsonl_files(&root, &mut paths)?;
    let mut sessions = Vec::new();
    for path in paths {
        let path = match validate_session_path_in_root(&root, &path) { Ok(path) => path, Err(_) => continue };
        match parse_codex_session(&path) {
            Ok(Some(session)) => sessions.push(session),
            Ok(None) => {},
            Err(error) => log::warn!("跳过无法解析的 Codex 会话 {}: {error}", path.display()),
        }
    }
    Ok((sessions, SessionProviderStatus {
        provider: SessionProvider::Codex,
        status: "available".to_string(),
        detail: "Codex 本地会话可用".to_string(),
        root_path: Some(root.display().to_string()),
    }))
}

fn claude_code_session_root() -> PathBuf {
    config::get_claude_config_dir().join("projects")
}

fn codex_session_root() -> PathBuf {
    config::get_codex_config_dir().join("sessions")
}


fn collect_jsonl_files(directory: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
    let entries = fs::read_dir(directory)
        .map_err(|error| AppError::Io(format!("读取会话目录 {} 失败: {error}", directory.display())))?;
    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(error) => {
                log::warn!("跳过无法读取的会话目录项: {error}");
                continue;
            }
        };
        let path = entry.path();
        if entry
            .file_type()
            .map(|file_type| file_type.is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        if path.is_dir() {
            if let Err(error) = collect_jsonl_files(&path, files) {
                log::warn!("跳过无法扫描的会话子目录 {}: {error}", path.display());
            }
            continue;
        }
        let is_jsonl = path.extension().and_then(|value| value.to_str()) == Some("jsonl");
        let is_agent = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("agent-"));
        if is_jsonl && !is_agent {
            files.push(path);
        }
    }
    Ok(())
}

fn parse_claude_code_session(path: &Path) -> AppResult<Option<SessionMeta>> {
    let file = File::open(path)
        .map_err(|error| AppError::Io(format!("打开会话 {} 失败: {error}", path.display())))?;
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
    }))
}

fn parse_codex_session(path: &Path) -> AppResult<Option<SessionMeta>> {
    let mut session = parse_claude_code_session(path)?;
    if let Some(session) = &mut session {
        session.provider = SessionProvider::Codex;
        session.resume_command = Some(format!("codex resume {}", session.session_id));
    }
    Ok(session)
}

fn load_claude_code_messages(path: &Path) -> AppResult<Vec<SessionMessage>> {
    let file = File::open(path)
        .map_err(|error| AppError::Io(format!("打开会话 {} 失败: {error}", path.display())))?;
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
    let root = root
        .canonicalize()
        .map_err(|error| AppError::Path(format!("无法解析会话根目录 {}: {error}", root.display())))?;
    let source = source
        .canonicalize()
        .map_err(|error| AppError::Path(format!("无法解析会话文件 {}: {error}", source.display())))?;
    if !source.starts_with(&root)
        || source.extension().and_then(|value| value.to_str()) != Some("jsonl")
    {
        return Err(AppError::Path(format!(
            "会话文件不在允许的目录内: {}",
            source.display()
        )));
    }
    Ok(source)
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
}
