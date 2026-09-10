fn scan_cline_sessions() -> (Vec<SessionMeta>, SessionProviderStatus) {
    let root = crate::config::cline::cline_sessions_dir();
    let db_found = crate::config::cline::cline_sessions_db_candidates()
        .into_iter()
        .any(|path| path.is_file());
    let available = root.is_dir() || db_found;
    let status = SessionProviderStatus {
        provider: SessionProvider::Cline,
        status: if available { "available" } else { "not_found" }.to_string(),
        detail: if available {
            format!("发现 Cline 会话目录 ({})", root.display())
        } else {
            "未找到 Cline 会话目录".to_string()
        },
        root_path: Some(root.to_string_lossy().into_owned()),
    };
    let items = crate::coding::cline::session::scan_cline_sessions().unwrap_or_default();
    let metas = items
        .into_iter()
        .map(|item| SessionMeta {
            provider: SessionProvider::Cline,
            session_id: item.id,
            title: item.title,
            summary: item.model.map(|model| format!("Model: {model}")),
            project_dir: item.project_dir,
            created_at: item.created_at,
            last_active_at: item.last_active_at,
            source_path: item.file_path,
            resume_command: None,
            pinned: false,
        })
        .collect();
    (metas, status)
}

fn load_cline_messages(source_path: &str) -> AppResult<Vec<SessionMessage>> {
    let root = crate::config::cline::cline_config_dir();
    let source = validate_session_path_in_root(&root, Path::new(source_path))?;
    let messages = crate::coding::cline::session::load_cline_messages(&source)?;
    Ok(messages
        .into_iter()
        .map(|message| SessionMessage {
            role: message.role,
            content: message.content,
            timestamp: message.timestamp,
        })
        .collect())
}

fn collect_dsh_session_paths(directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() { continue }
        if kind.is_dir() {
            collect_dsh_session_paths(&path, paths);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl.zstd") {
            paths.push(path);
        }
    }
}

fn session_root(provider: SessionProvider) -> AppResult<PathBuf> {
    match provider {
        SessionProvider::ClaudeCode => Ok(claude_code_session_root()),
        SessionProvider::Codex => Ok(codex_session_root()),
        SessionProvider::OpenCode => Err(AppError::Config(
            "OpenCode 会话暂不支持归档、回收站与导入操作".to_string(),
        )),
        SessionProvider::Pi => Ok(crate::coding::pi::config::get_pi_dir().join("sessions")),
        SessionProvider::Dsh => Ok(crate::config::get_dsh_config_dir().join("sessions")),
        SessionProvider::Cline => Err(AppError::Config("Cline 会话暂不支持归档、回收站与导入操作".to_string())),
    }
}

pub(crate) fn parse_session(provider: SessionProvider, path: &Path) -> AppResult<Option<SessionMeta>> {
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
        SessionProvider::Dsh => {
            let mtime = path.metadata().ok().and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64).unwrap_or(0);
            Ok(session_meta_from_path(SessionProvider::Dsh, path, mtime))
        }
        SessionProvider::Cline => Ok(None),
    }
}

pub(crate) fn validated_session(provider: SessionProvider, source_path: &str) -> AppResult<(PathBuf, PathBuf)> {
    let root = session_root(provider)?;
    let source = validate_session_path_in_root(&root, Path::new(source_path))?;
    let relative = relative_to_root(&source, &root)?;
    Ok((source, relative))
}

pub(crate) fn session_manifest(provider: SessionProvider, meta: &SessionMeta, relative: PathBuf, content: &[u8]) -> SessionArchiveManifest {
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

pub(crate) fn import_target(provider: SessionProvider, relative_path: &str) -> AppResult<PathBuf> {
    let relative = safe_archive_relative_path(relative_path)?;
    let root = session_root(provider)?;
    fs::create_dir_all(&root)?;
    let root = simplified_path(&root.canonicalize()?);
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
        SessionProvider::Dsh => "dsh",
        SessionProvider::Cline => "cline",
        _ => "claude-code",
    };
    config::get_app_config_dir().join("session-trash").join(target)
}

fn validated_code_session(source_path: &str) -> AppResult<(PathBuf, PathBuf)> {
    let root = claude_code_session_root();
    let source = validate_session_path_in_root(&root, Path::new(source_path))?;
    let relative = relative_to_root(&source, &root)?;
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

pub(crate) fn write_batch_session_archive(
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
pub(crate) fn resolve_export_dir(destination_dir: Option<&str>, default_subdir: &str) -> AppResult<PathBuf> {
    match destination_dir.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => {
            let path = simplified_path(Path::new(path));
            let metadata = fs::metadata(&path).map_err(|error| {
                AppError::Path(format!("无法访问导出目录 {}: {error}", path.display()))
            })?;
            if !metadata.is_dir() {
                return Err(AppError::Path(format!("导出位置不是目录: {}", path.display())));
            }
            let canonical = path.canonicalize().map_err(|error| {
                AppError::Path(format!("无法解析导出目录 {}: {error}", path.display()))
            })?;
            Ok(simplified_path(&canonical))
        }
        None => {
            let dir = config::get_app_config_dir().join(default_subdir);
            fs::create_dir_all(&dir)?;
            Ok(simplified_path(&dir))
        }
    }
}

