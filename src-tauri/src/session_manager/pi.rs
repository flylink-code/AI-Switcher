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

