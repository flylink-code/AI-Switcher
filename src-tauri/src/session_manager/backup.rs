pub const SESSION_BACKUP_DIRECTORY_KEY: &str = "session_backup_directory";

pub fn archive_provider_slug(provider: SessionProvider) -> &'static str {
    match provider {
        SessionProvider::Codex => "codex",
        SessionProvider::ClaudeCode => "claude-code",
        SessionProvider::Pi => "pi",
        SessionProvider::Dsh => "dsh",
        SessionProvider::OpenCode => "opencode",
        SessionProvider::Cline => "cline",
    }
}

pub fn is_auto_backup_filename(name: &str) -> bool {
    name.contains("-auto-backup-")
}

pub fn file_mtime_secs(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

pub fn is_within_active_window(mtime_secs: i64, active_days: u32) -> bool {
    if active_days == 0 {
        return true;
    }
    let now = chrono::Utc::now().timestamp();
    now.saturating_sub(mtime_secs) <= i64::from(active_days).saturating_mul(86_400)
}

pub fn default_session_backup_dir() -> PathBuf {
    config::get_app_config_dir().join("session-backups")
}

pub fn get_configured_session_backup_dir(conn: &rusqlite::Connection) -> AppResult<PathBuf> {
    use crate::database::dao::settings::get_setting;
    if let Some(custom) = get_setting(conn, SESSION_BACKUP_DIRECTORY_KEY)? {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            if let Some(usable) = config::paths::usable_local_absolute_path(trimmed) {
                let path = simplified_path(&usable);
                if !path.exists() {
                    let _ = fs::create_dir_all(&path);
                }
                if path.is_dir() {
                    return Ok(path);
                }
            } else {
                rewrite_foreign_session_backup_directory(conn)?;
            }
        }
    }
    let default_dir = simplified_path(&default_session_backup_dir());
    if !default_dir.exists() {
        let _ = fs::create_dir_all(&default_dir);
    }
    Ok(default_dir)
}

pub fn set_configured_session_backup_dir(conn: &rusqlite::Connection, path: &str) -> AppResult<String> {
    use crate::database::dao::settings::set_setting;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return reset_configured_session_backup_dir(conn);
    }
    let usable = config::paths::usable_local_absolute_path(trimmed).ok_or_else(|| {
        AppError::Path(format!(
            "备份目录必须是本机绝对路径，不能使用另一操作系统的路径: {trimmed}"
        ))
    })?;
    let target = simplified_path(&usable);
    if !target.exists() {
        fs::create_dir_all(&target).map_err(|error| {
            AppError::Path(format!("创建备份目录失败 {}: {error}", target.display()))
        })?;
    }
    let canonical = target.canonicalize().map_err(|error| {
        AppError::Path(format!("无法解析备份目录 {}: {error}", target.display()))
    })?;
    if !canonical.is_dir() {
        return Err(AppError::Path(format!("指定路径不是有效目录: {}", canonical.display())));
    }
    let stored = simplified_path(&canonical);
    let string_path = stored.to_string_lossy().into_owned();
    set_setting(conn, SESSION_BACKUP_DIRECTORY_KEY, &string_path)?;
    Ok(string_path)
}

/// Drop a session-backup directory that cannot be used on this OS (for example a
/// Windows `J:\...` path imported onto Linux) so the local default applies.
pub fn rewrite_foreign_session_backup_directory(conn: &rusqlite::Connection) -> AppResult<()> {
    use crate::database::dao::settings::{get_setting, set_setting};
    let Some(custom) = get_setting(conn, SESSION_BACKUP_DIRECTORY_KEY)? else {
        return Ok(());
    };
    let trimmed = custom.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if config::paths::usable_local_absolute_path(trimmed).is_some() {
        return Ok(());
    }
    set_setting(conn, SESSION_BACKUP_DIRECTORY_KEY, "")?;
    Ok(())
}

pub fn reset_configured_session_backup_dir(conn: &rusqlite::Connection) -> AppResult<String> {
    use crate::database::dao::settings::set_setting;
    let default_dir = simplified_path(&default_session_backup_dir());
    if !default_dir.exists() {
        let _ = fs::create_dir_all(&default_dir);
    }
    set_setting(conn, SESSION_BACKUP_DIRECTORY_KEY, "")?;
    Ok(default_dir.to_string_lossy().into_owned())
}

pub fn collect_all_session_paths_for_provider(provider: SessionProvider) -> AppResult<Vec<String>> {
    match provider {
        SessionProvider::ClaudeCode => {
            let (paths, _, _, _) = collect_claude_code_session_paths()?;
            Ok(paths.into_iter().map(|(p, _)| p.to_string_lossy().into_owned()).collect())
        }
        SessionProvider::Codex => {
            let (paths, _, _, _) = collect_codex_session_paths()?;
            Ok(paths.into_iter().map(|(p, _)| p.to_string_lossy().into_owned()).collect())
        }
        SessionProvider::Pi => {
            let root = crate::coding::pi::config::get_pi_dir().join("sessions");
            let mut paths = Vec::new();
            if root.is_dir() {
                collect_jsonl_files(&root, &mut paths)?;
            }
            Ok(paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
        }
        SessionProvider::Dsh => {
            let root = crate::config::get_dsh_config_dir().join("sessions");
            let mut paths = Vec::new();
            if root.is_dir() {
                collect_dsh_session_paths(&root, &mut paths);
            }
            Ok(paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
        }
        SessionProvider::OpenCode => {
            Err(AppError::Config("OpenCode 会话暂不支持批量文件归档备份".to_string()))
        }
        SessionProvider::Cline => {
            Err(AppError::Config("Cline 会话暂不支持批量文件归档备份".to_string()))
        }
    }
}

pub fn backup_all_sessions(
    provider: SessionProvider,
    destination_dir: Option<&str>,
) -> AppResult<SessionBatchExportInfo> {
    let source_paths = collect_all_session_paths_for_provider(provider)?;
    if source_paths.is_empty() {
        return Err(AppError::Config(format!(
            "未发现 {:?} 的本地会话文件，无法执行备份",
            provider
        )));
    }
    let mut sessions = Vec::with_capacity(source_paths.len());
    for source_path in &source_paths {
        let (source, relative) = validated_session(provider, source_path)?;
        let content = fs::read(&source)?;
        let meta = parse_session(provider, &source)?
            .ok_or_else(|| AppError::Config("无法读取会话元数据".to_string()))?;
        sessions.push((session_manifest(provider, &meta, relative, &content), content));
    }
    let created_at = chrono::Utc::now().timestamp_millis();
    let dir = resolve_export_dir(destination_dir, "session-backups")?;
    let name = archive_provider_slug(provider);
    let archive_path = dir.join(format!("{name}-all-backup-{created_at}.zip"));
    write_batch_session_archive(&archive_path, created_at, &sessions)?;
    Ok(SessionBatchExportInfo {
        archive_path: archive_path.to_string_lossy().into_owned(),
        session_count: sessions.len(),
        created_at,
    })
}

/// Timed auto backup: only sessions inside the active window, skip unreadable files.
/// Returns `None` when there is nothing to archive.
pub fn backup_all_sessions_auto(
    provider: SessionProvider,
    destination_dir: Option<&str>,
    active_days: u32,
) -> AppResult<Option<SessionBatchExportInfo>> {
    let source_paths = match collect_all_session_paths_for_provider(provider) {
        Ok(paths) => paths,
        Err(error) => {
            log::warn!("自动会话备份跳过 {provider:?}: {error}");
            return Ok(None);
        }
    };
    let mut sessions = Vec::new();
    for source_path in &source_paths {
        let Ok((source, relative)) = validated_session(provider, source_path) else {
            continue;
        };
        if let Some(mtime) = file_mtime_secs(&source) {
            if !is_within_active_window(mtime, active_days) {
                continue;
            }
        }
        let Ok(content) = fs::read(&source) else {
            continue;
        };
        let Ok(Some(meta)) = parse_session(provider, &source) else {
            continue;
        };
        sessions.push((session_manifest(provider, &meta, relative, &content), content));
    }
    if sessions.is_empty() {
        return Ok(None);
    }
    let created_at = chrono::Utc::now().timestamp_millis();
    let dir = resolve_export_dir(destination_dir, "session-backups")?;
    let name = archive_provider_slug(provider);
    let archive_path = dir.join(format!("{name}-auto-backup-{created_at}.zip"));
    write_batch_session_archive(&archive_path, created_at, &sessions)?;
    Ok(Some(SessionBatchExportInfo {
        archive_path: archive_path.to_string_lossy().into_owned(),
        session_count: sessions.len(),
        created_at,
    }))
}

pub fn prune_auto_session_backups(
    dir: &Path,
    provider: SessionProvider,
    keep: usize,
) -> AppResult<usize> {
    if keep == 0 || !dir.is_dir() {
        return Ok(0);
    }
    let slug = archive_provider_slug(provider);
    let prefix = format!("{slug}-auto-backup-");
    let mut archives: Vec<(i64, PathBuf)> = Vec::new();
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("zip") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !name.starts_with(&prefix) {
            continue;
        }
        let stamp = name
            .trim_start_matches(&prefix)
            .trim_end_matches(".zip")
            .parse::<i64>()
            .unwrap_or_else(|_| file_mtime_secs(&path).unwrap_or(0));
        archives.push((stamp, path));
    }
    archives.sort_by(|left, right| right.0.cmp(&left.0));
    let mut removed = 0;
    for (_, path) in archives.into_iter().skip(keep) {
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn list_session_backups(
    provider: Option<SessionProvider>,
    backup_dir: Option<&str>,
) -> AppResult<Vec<SessionBackupArchiveInfo>> {
    let dir = resolve_export_dir(backup_dir, "session-backups")?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut archives = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("zip") {
            continue;
        }
        let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if let Ok((batch, _)) = read_batch_session_archive(&path) {
            let archive_provider = batch
                .sessions
                .first()
                .map(|s| s.provider)
                .unwrap_or(SessionProvider::ClaudeCode);
            if provider.is_none() || provider == Some(archive_provider) {
                archives.push(SessionBackupArchiveInfo {
                    archive_path: path.to_string_lossy().into_owned(),
                    filename: filename.clone(),
                    provider: archive_provider,
                    session_count: batch.sessions.len(),
                    created_at: batch.created_at,
                    file_size,
                    is_batch: true,
                    is_auto: is_auto_backup_filename(&filename),
                });
            }
        } else if let Ok((manifest, _)) = read_session_archive(&path) {
            if provider.is_none() || provider == Some(manifest.provider) {
                archives.push(SessionBackupArchiveInfo {
                    archive_path: path.to_string_lossy().into_owned(),
                    filename: filename.clone(),
                    provider: manifest.provider,
                    session_count: 1,
                    created_at: manifest.created_at,
                    file_size,
                    is_batch: false,
                    is_auto: is_auto_backup_filename(&filename),
                });
            }
        }
    }
    archives.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(archives)
}

pub fn restore_session_backup(
    provider: SessionProvider,
    archive_path: &str,
    overwrite: bool,
) -> AppResult<SessionBatchRestoreResult> {
    let path = Path::new(archive_path);
    if !path.is_file() {
        return Err(AppError::Config(format!("备份文件不存在: {archive_path}")));
    }

    let mut restored_count = 0;
    let mut skipped_count = 0;

    if is_batch_session_archive(path)? {
        let (batch, contents) = read_batch_session_archive(path)?;
        for (manifest, content) in batch.sessions.iter().zip(contents.iter()) {
            validate_manifest_provider(provider, manifest)?;
            let target = import_target(provider, &manifest.relative_path)?;
            if target.exists() {
                let existing_sha = hex::encode(Sha256::digest(fs::read(&target)?));
                if existing_sha == manifest.content_sha256 {
                    skipped_count += 1;
                    continue;
                }
                if !overwrite {
                    skipped_count += 1;
                    continue;
                }
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            crate::config::atomic_write(&target, content)?;
            restored_count += 1;
        }
    } else {
        let (manifest, content) = read_session_archive(path)?;
        validate_manifest_provider(provider, &manifest)?;
        let target = import_target(provider, &manifest.relative_path)?;
        if target.exists() {
            let existing_sha = hex::encode(Sha256::digest(fs::read(&target)?));
            if existing_sha == manifest.content_sha256 {
                skipped_count += 1;
            } else if !overwrite {
                skipped_count += 1;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                crate::config::atomic_write(&target, &content)?;
                restored_count += 1;
            }
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            crate::config::atomic_write(&target, &content)?;
            restored_count += 1;
        }
    }

    if provider == SessionProvider::Codex && restored_count > 0 {
        let _ = crate::config::codex_provider_sync::sync_to_managed_provider();
    }

    Ok(SessionBatchRestoreResult {
        restored_count,
        skipped_count,
        total_count: restored_count + skipped_count,
        message: format!(
            "已恢复 {} 个会话，跳过 {} 个会话",
            restored_count, skipped_count
        ),
    })
}

fn safe_archive_relative_path(value: &str) -> AppResult<PathBuf> {
    let path = Path::new(value);
    let valid_ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|ext| ext == "jsonl" || ext == "zstd" || ext == "json")
        .unwrap_or(false);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
        || !valid_ext
    {
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
        SessionProvider::Codex | SessionProvider::OpenCode | SessionProvider::Pi | SessionProvider::Dsh | SessionProvider::Cline => None,
    };
    let resume = match provider {
        SessionProvider::ClaudeCode => resume_command(&session_id),
        SessionProvider::Codex => Some(format!("codex resume {session_id}")),
        // OpenCode / Pi 元数据走 Materialized 路径，不会经过这里。
        SessionProvider::OpenCode => Some(format!("opencode -s {session_id}")),
        SessionProvider::Pi => Some(format!("pi --resume {session_id}")),
        SessionProvider::Dsh | SessionProvider::Cline => None,
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

