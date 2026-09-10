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

