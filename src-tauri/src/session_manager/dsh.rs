fn scan_dsh_sessions() -> (Vec<SessionMeta>, SessionProviderStatus) {
    let root = crate::config::get_dsh_config_dir().join("sessions");
    let status = SessionProviderStatus {
        provider: SessionProvider::Dsh,
        status: if root.is_dir() { "available" } else { "not_found" }.to_string(),
        detail: if root.is_dir() { format!("发现 DeepSeek Harness 会话目录 ({})", root.display()) } else { "未找到 DeepSeek Harness 会话目录".to_string() },
        root_path: Some(root.to_string_lossy().into_owned()),
    };
    let mut paths = Vec::new();
    collect_dsh_session_paths(&root, &mut paths);
    let mut sessions = Vec::new();
    for path in paths {
        let Ok(events) = crate::usage::session_usage_dsh::read_dsh_events(&path) else { continue };
        let header = events.iter().find(|event| event.get("type").and_then(Value::as_str) == Some("session"));
        let session_id = header.and_then(|event| event.get("id")).and_then(Value::as_str)
            .or_else(|| path.parent().and_then(|parent| parent.file_name()).and_then(|value| value.to_str()))
            .unwrap_or_default().to_string();
        if session_id.is_empty() { continue }
        let created_at = header.and_then(|event| event.get("createdAt")).and_then(Value::as_i64);
        let project_dir = header.and_then(|event| event.get("cwd")).and_then(Value::as_str).map(str::to_string);
        let title = events.iter().rev().find_map(|event| {
            (event.get("type").and_then(Value::as_str) == Some("session/title"))
                .then(|| event.pointer("/data/title").and_then(Value::as_str).map(str::to_string))
                .flatten()
        });
        let last_active_at = events.iter().rev().find_map(|event| event.get("time").and_then(Value::as_i64));
        let model = events.iter().rev().find_map(|event| event.pointer("/data/message/source/model").and_then(Value::as_str));
        sessions.push(SessionMeta {
            provider: SessionProvider::Dsh,
            session_id,
            title,
            summary: model.map(|model| format!("Model: {model}")),
            project_dir,
            created_at,
            last_active_at,
            source_path: path.to_string_lossy().into_owned(),
            resume_command: None,
            pinned: false,
        });
    }
    (sessions, status)
}

