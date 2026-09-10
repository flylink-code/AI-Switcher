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
    let root = simplified_path(&root);
    let source = simplified_path(&source);
    if relative_to_root(&source, &root).is_err() && !source.starts_with(&root) {
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

/// Strip Windows extended-length prefixes so stored and compared paths stay
/// usable by ShellExecute / Path::strip_prefix.
pub(crate) fn simplified_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let stripped = strip_windows_path_prefix(&raw);
    #[cfg(windows)]
    {
        PathBuf::from(stripped.replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(stripped)
    }
}

fn relative_under_simplified(source: &Path, root: &Path) -> Option<PathBuf> {
    let source = simplified_path(source);
    let root = simplified_path(root);
    if let Ok(relative) = source.strip_prefix(&root) {
        if !relative.as_os_str().is_empty() {
            return Some(relative.to_path_buf());
        }
    }
    let source_key = normalize_path_key(&source);
    let root_key = normalize_path_key(&root);
    let rest = source_key.strip_prefix(&root_key)?;
    if rest.is_empty() {
        return None;
    }
    if !rest.starts_with('\\') && !rest.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(
        rest.trim_start_matches(['\\', '/'])
            .replace('\\', std::path::MAIN_SEPARATOR_STR),
    ))
}

pub(crate) fn relative_to_root(source: &Path, root: &Path) -> AppResult<PathBuf> {
    if let Some(relative) = relative_under_simplified(source, root) {
        return Ok(relative);
    }
    let source = source
        .canonicalize()
        .map(|path| simplified_path(&path))
        .unwrap_or_else(|_| simplified_path(source));
    let root = root
        .canonicalize()
        .map(|path| simplified_path(&path))
        .unwrap_or_else(|_| simplified_path(root));
    relative_under_simplified(&source, &root)
        .ok_or_else(|| AppError::Path("会话相对路径无效".to_string()))
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


