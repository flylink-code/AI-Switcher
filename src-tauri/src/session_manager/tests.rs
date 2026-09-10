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
        crate::config::paths::with_isolated_codex_home(dir.path(), || {
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

            assert_eq!(result.total, 1, "providers={:?}", result.providers);
            assert_eq!(result.sessions[0].provider, SessionProvider::Codex);
            assert!(result.sessions[0]
                .source_path
                .replace('\\', "/")
                .ends_with(
                    "rollout-2026-08-05T12-00-00-019f8d32-4e9b-7551-acde-45e4c9a58e0b.jsonl"
                ));
        });
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
    fn simplified_path_strips_windows_extended_prefix() {
        assert_eq!(
            simplified_path(Path::new(r"\\?\J:\Temp\aiswitcher")),
            PathBuf::from(r"J:\Temp\aiswitcher")
        );
        assert_eq!(
            simplified_path(Path::new(r"//?/J:\Temp\aiswitcher")),
            PathBuf::from(r"J:\Temp\aiswitcher")
        );
        assert_eq!(
            simplified_path(Path::new(r"J:\Temp\aiswitcher")),
            PathBuf::from(r"J:\Temp\aiswitcher")
        );
    }

    #[test]
    fn relative_to_root_accepts_mixed_windows_verbatim_prefix() {
        let relative = relative_to_root(
            Path::new(r"C:\Users\admin\.claude\projects\acme\foo.jsonl"),
            Path::new(r"\\?\C:\Users\admin\.claude\projects"),
        )
        .expect("relative path");
        assert_eq!(relative.file_name().unwrap(), "foo.jsonl");
        let display = relative.to_string_lossy();
        assert!(
            display.contains("acme"),
            "expected project folder in relative path, got {display}"
        );
    }

    fn settings_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn backup_dir_round_trip_strips_windows_extended_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let conn = settings_conn();
        let prefixed = format!(r"\\?\{}", simplified_path(dir.path()).display());
        let stored = set_configured_session_backup_dir(&conn, &prefixed).unwrap();
        assert!(
            !stored.contains(r"\\?\"),
            "stored backup dir still has verbatim prefix: {stored}"
        );
        let loaded = get_configured_session_backup_dir(&conn).unwrap();
        assert!(
            !loaded.to_string_lossy().contains(r"\\?\"),
            "loaded backup dir still has verbatim prefix: {}",
            loaded.display()
        );
        assert_eq!(Path::new(&stored), loaded.as_path());
    }

    #[cfg(unix)]
    #[test]
    fn foreign_windows_backup_dir_is_ignored_and_cleared() {
        let conn = settings_conn();
        crate::database::dao::settings::set_setting(
            &conn,
            SESSION_BACKUP_DIRECTORY_KEY,
            r"J:\Temp\aiswitcher\session-backups",
        )
        .unwrap();
        let ghost = PathBuf::from(r"J:\Temp\aiswitcher\session-backups");
        let existed_before = ghost.exists();
        let loaded = get_configured_session_backup_dir(&conn).unwrap();
        assert_eq!(loaded, simplified_path(&default_session_backup_dir()));
        if !existed_before {
            assert!(
                !ghost.exists(),
                "must not mkdir a Windows path as a literal folder on Unix"
            );
        }
        let stored = crate::database::dao::settings::get_setting(&conn, SESSION_BACKUP_DIRECTORY_KEY)
            .unwrap()
            .unwrap_or_default();
        assert!(stored.is_empty(), "stale Windows backup dir should be cleared, got {stored}");
    }

    #[cfg(unix)]
    #[test]
    fn rewrite_foreign_session_backup_directory_clears_windows_path() {
        let conn = settings_conn();
        crate::database::dao::settings::set_setting(
            &conn,
            SESSION_BACKUP_DIRECTORY_KEY,
            r"\\?\J:\Temp\aiswitcher",
        )
        .unwrap();
        rewrite_foreign_session_backup_directory(&conn).unwrap();
        let stored = crate::database::dao::settings::get_setting(&conn, SESSION_BACKUP_DIRECTORY_KEY)
            .unwrap()
            .unwrap_or_default();
        assert!(stored.is_empty());
    }

    #[test]
    fn rewrite_keeps_native_session_backup_directory() {
        let dir = tempfile::tempdir().unwrap();
        let native = simplified_path(dir.path());
        let native_str = native.to_string_lossy().into_owned();
        let conn = settings_conn();
        crate::database::dao::settings::set_setting(
            &conn,
            SESSION_BACKUP_DIRECTORY_KEY,
            &native_str,
        )
        .unwrap();
        rewrite_foreign_session_backup_directory(&conn).unwrap();
        let stored = crate::database::dao::settings::get_setting(&conn, SESSION_BACKUP_DIRECTORY_KEY)
            .unwrap()
            .unwrap_or_default();
        assert_eq!(stored, native_str);
        let loaded = get_configured_session_backup_dir(&conn).unwrap();
        assert_eq!(loaded, native);
    }

    #[test]
    fn set_session_backup_dir_rejects_foreign_os_path() {
        let conn = settings_conn();
        #[cfg(unix)]
        {
            let error = set_configured_session_backup_dir(&conn, r"J:\Temp\aiswitcher").unwrap_err();
            assert!(error.to_string().contains("本机绝对路径"));
        }
        #[cfg(windows)]
        {
            let error = set_configured_session_backup_dir(&conn, "/home/user/.claude-switcher").unwrap_err();
            assert!(error.to_string().contains("本机绝对路径"));
        }
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
    fn file_archive_backup_is_unsupported_for_opencode_and_cline() {
        let open = collect_all_session_paths_for_provider(SessionProvider::OpenCode).unwrap_err();
        assert!(open.to_string().contains("OpenCode"));
        let cline = collect_all_session_paths_for_provider(SessionProvider::Cline).unwrap_err();
        assert!(cline.to_string().contains("Cline"));
    }

    #[test]
    fn list_session_backups_ignores_non_archive_files() {
        let dir = tempfile::tempdir().unwrap();
        File::create(dir.path().join("readme.txt")).unwrap();
        File::create(dir.path().join("not-a-session.zip")).unwrap();
        let archives = list_session_backups(None, Some(dir.path().to_str().unwrap())).unwrap();
        assert!(archives.is_empty());
    }

    #[test]
    fn prune_auto_backups_keeps_manual_zip() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pi-all-backup-1.zip"), b"manual").unwrap();
        fs::write(dir.path().join("pi-auto-backup-1.zip"), b"a").unwrap();
        fs::write(dir.path().join("pi-auto-backup-2.zip"), b"b").unwrap();
        fs::write(dir.path().join("pi-auto-backup-3.zip"), b"c").unwrap();
        let removed = prune_auto_session_backups(dir.path(), SessionProvider::Pi, 2).unwrap();
        assert_eq!(removed, 1);
        assert!(dir.path().join("pi-all-backup-1.zip").is_file());
        assert!(dir.path().join("pi-auto-backup-3.zip").is_file());
        assert!(!dir.path().join("pi-auto-backup-1.zip").is_file());
        assert!(is_auto_backup_filename("claude-code-auto-backup-9.zip"));
        assert!(!is_auto_backup_filename("claude-code-all-backup-9.zip"));
    }
}
