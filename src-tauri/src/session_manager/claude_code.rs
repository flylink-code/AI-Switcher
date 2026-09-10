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

