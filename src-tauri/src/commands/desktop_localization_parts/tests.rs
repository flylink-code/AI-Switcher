// 中文化修复的单测：全部跑在临时目录搭的假 resources 树上，不碰真实 Claude Desktop，
// 也不启动/结束任何进程。由 desktop_localization.rs 在 `#[cfg(test)]` 下 `include!`。

use tempfile::tempdir;

const SAMPLE_REPLACEMENTS: &str =
    r#"[["Add","添加"],["Copy","复制"],["Check","检查"],["tokens","令牌"],["matches","匹配"]]"#;

fn two_whitelists() -> String {
    format!(
        "var w_={BASE_LANGUAGES}];var M_={BASE_LANGUAGES}];function N_(e){{return M_.includes(e)}}"
    )
}

/// 原版 bundle：两份白名单 + 图标名 + 逻辑词 + 可翻译文案 + 合法的 CJK name。
fn pristine_js() -> String {
    format!(
        "{}const a={{icon:\"Add\",href:ii}},b={{icon:Re?\"Check\":\"Copy\"}},c=\"tokens\"in e&&e.tokens,d={{label:\"Add\"}},n={{name:\"简体中文\"}};",
        two_whitelists()
    )
}

/// 旧版逻辑改坏的 bundle：只补了第一份白名单，图标名被翻成中文，逻辑词被替换。
fn corrupted_js() -> String {
    format!(
        "var w_={BASE_LANGUAGES},\"zh-CN\"];var M_={BASE_LANGUAGES}];function N_(e){{return M_.includes(e)}}const a={{icon:\"添加\",href:ii}},b={{icon:Re?\"Check\":\"复制\"}},c=\"令牌\"in e&&e.tokens,d={{label:\"添加\"}};"
    )
}

/// 只有腐败标记：白名单一份都没注册（没有 zh-CN），也没有 zh 资源文件。
fn markers_only_corrupted_js() -> String {
    format!(
        "{}const a={{icon:\"添加\",href:ii}},c=\"令牌\"in e&&e.tokens;",
        two_whitelists()
    )
}

fn fake_install(root: &Path) -> LocalizationInstall {
    let app_path = root.join("Claude");
    let resources_path = app_path.join("resources");
    fs::create_dir_all(frontend_assets_dir(&resources_path)).unwrap();
    LocalizationInstall {
        kind: "unpackaged".to_string(),
        app_path,
        resources_path,
        exe_path: root.join("Claude.exe"),
        version: "1.2.3".to_string(),
        multiple_installs: false,
    }
}

fn fake_pack(root: &Path, replacements_json: &str) -> ValidatedPack {
    let resources = root.join("pack").join("resources");
    fs::create_dir_all(&resources).unwrap();
    fs::write(resources.join(FRONTEND_FILE), br#"{"k":"v"}"#).unwrap();
    fs::write(resources.join(DESKTOP_FILE), br#"{"k":"v"}"#).unwrap();
    fs::write(resources.join(STATSIG_FILE), br#"{"k":"v"}"#).unwrap();
    fs::write(resources.join(HARDCODED_FILE), replacements_json).unwrap();
    validate_pack(&root.join("pack")).unwrap()
}

fn write_dynamic_en_us(install: &LocalizationInstall) {
    let path = dynamic_en_us_path(&install.resources_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, br#"{"en":"US"}"#).unwrap();
}

fn locale_path(root: &Path) -> PathBuf {
    root.join("profile").join("Claude").join("config.json")
}

fn bundle_path(install: &LocalizationInstall) -> PathBuf {
    frontend_assets_dir(&install.resources_path).join("bundle.js")
}

fn replacements(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(source, target)| (source.to_string(), target.to_string()))
        .collect()
}

// ---------- 步骤 1：补丁引擎 ----------

#[test]
fn register_language_patches_every_whitelist_occurrence() {
    let text = format!("a;{BASE_LANGUAGES},\"zh-CN\"];b;{BASE_LANGUAGES}];c");
    let (patched, registration) = register_language(&text);
    assert_eq!(registration.lists_found, 2);
    assert_eq!(registration.lists_changed, 1);
    assert_eq!(patched.matches(",\"zh-CN\"]").count(), 2);
    assert_eq!(count_language_lists(&patched), (2, 2));
}

#[test]
fn register_language_is_idempotent_across_multiple_lists() {
    let (once, _) = register_language(&two_whitelists());
    let (twice, registration) = register_language(&once);
    assert_eq!(once, twice);
    assert_eq!(registration.lists_found, 2);
    assert_eq!(registration.lists_changed, 0);
}

#[test]
fn register_language_strips_other_chinese_variants() {
    let text = format!("{BASE_LANGUAGES},\"zh-TW\",\"zh-HK\"]");
    let (patched, _) = register_language(&text);
    assert!(patched.ends_with(",\"zh-CN\"]"));
    assert!(!patched.contains("zh-TW"));
    assert!(!patched.contains("zh-HK"));
    assert_eq!(patched.matches("zh-CN").count(), 1);
}

#[test]
fn count_language_lists_reports_unpatched_list() {
    let text = format!("{BASE_LANGUAGES},\"zh-CN\"];{BASE_LANGUAGES}]");
    assert_eq!(count_language_lists(&text), (2, 1));
    assert_eq!(count_language_lists("nothing"), (0, 0));
}

#[test]
fn lowercase_word_sources_are_skipped_on_load() {
    let root = tempdir().unwrap();
    let path = root.path().join("hardcoded.json");
    fs::write(
        &path,
        r#"[["tokens","令牌"],["matches","匹配"],["seconds","秒"],["days","天"],["minutes","分钟"],["Add","添加"],["Copy","复制"]]"#,
    )
    .unwrap();
    let loaded = load_hardcoded_replacements(&path).unwrap();
    let mut sources = loaded.iter().map(|item| item.0.as_str()).collect::<Vec<_>>();
    sources.sort_unstable();
    assert_eq!(sources, vec!["Add", "Copy"]);
}

#[test]
fn hardcoded_replacements_keep_icon_names() {
    let protected = [
        r#"{icon:"Add",href:ii}"#,
        r#"{icon:Re?"Check":"Copy"}"#,
        r#"{primaryActionCompactIcon:"Copy"}"#,
        r#"{openIcon:"Add"}"#,
        r#"{triggerIcon:x?"Add":"Add"}"#,
        r#"{icon:i="Add"}"#,
        r#"{icon:f??"Add"}"#,
        r#"{icon:e.length===0?"A":"Add"}"#,
        r#"{name:"Add"}"#,
        r#"{type:"Add"}"#,
    ];
    let translated = [
        (r#"{label:"Add"}"#, r#"{label:"添加"}"#),
        (r#"{children:"Add"}"#, r#"{children:"添加"}"#),
        (r#"{title:x?"a":"Add"}"#, r#"{title:x?"a":"添加"}"#),
    ];
    let mut text = protected.join(";");
    for (source, _) in translated {
        text.push(';');
        text.push_str(source);
    }
    let rules = replacements(&[("Add", "添加"), ("Copy", "复制"), ("Check", "检查")]);
    let count = apply_hardcoded_replacements(&mut text, &rules);
    for item in protected {
        assert!(text.contains(item), "应保持原样: {item}\n实际: {text}");
    }
    for (_, expected) in translated {
        assert!(text.contains(expected), "应被翻译: {expected}\n实际: {text}");
    }
    assert_eq!(count, 3);
}

#[test]
fn icon_context_does_not_cross_property_boundaries() {
    let mut text = r#"{icon:foo,label:"Add"};{icon:"X",label:"Add"}"#.to_string();
    let count = apply_hardcoded_replacements(&mut text, &replacements(&[("Add", "添加")]));
    assert_eq!(count, 2);
    assert_eq!(text, r#"{icon:foo,label:"添加"};{icon:"X",label:"添加"}"#);
}

#[test]
fn hardcoded_replacements_do_not_touch_tokens_in_e() {
    let root = tempdir().unwrap();
    let path = root.path().join("hardcoded.json");
    fs::write(
        &path,
        r#"[["tokens","令牌"],["matches","匹配"],["days","天"],["seconds","秒"],["Add","添加"]]"#,
    )
    .unwrap();
    let loaded = load_hardcoded_replacements(&path).unwrap();
    let original = r#"if("tokens"in e&&e.tokens){x("days","seconds","matches")}label:"Add""#;
    let mut text = original.to_string();
    apply_hardcoded_replacements(&mut text, &loaded);
    assert_eq!(
        text,
        r#"if("tokens"in e&&e.tokens){x("days","seconds","matches")}label:"添加""#
    );
}

#[test]
fn scan_patch_state_flags_icon_cjk_and_token_corruption() {
    let mut scan = PatchScan::default();
    scan.add_text(&corrupted_js());
    assert!(scan.is_corrupted());
    assert_eq!(scan.icon_cjk, 2);
    assert_eq!(scan.token_marker, 1);
    assert_eq!((scan.lists_total, scan.lists_with_zh), (2, 1));
    assert!(!scan.all_lists_registered());
}

#[test]
fn scan_patch_state_is_clean_on_original_sample() {
    let mut scan = PatchScan::default();
    scan.add_text(&pristine_js());
    assert!(!scan.is_corrupted());
    assert_eq!(scan.lists_with_zh, 0);
    assert_eq!(scan.lists_total, 2);
}

#[test]
fn verify_patched_sources_rejects_new_icon_corruption() {
    let original = r#"{icon:"Add"}"#;
    assert!(verify_patched_sources(original, r#"{icon:"添加"}"#).is_err());
    assert!(verify_patched_sources(r#"x="tokens"in e"#, r#"x="令牌"in e"#).is_err());
}

#[test]
fn verify_patched_sources_allows_preexisting_cjk_name() {
    let original = r#"{name:"简体中文",label:"Add"}"#;
    let patched = r#"{name:"简体中文",label:"添加"}"#;
    assert!(verify_patched_sources(original, patched).is_ok());
}

// ---------- 步骤 2：collect_install_changes ----------

#[test]
fn collect_install_changes_adds_dynamic_catalog_copy() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    write_dynamic_en_us(&install);
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let pack = fake_pack(root.path(), SAMPLE_REPLACEMENTS);
    let plan = collect_install_changes(&install, &pack, &[], &JobLog::none()).unwrap();
    let dynamic = plan
        .changes
        .iter()
        .find(|change| change.target == dynamic_zh_cn_path(&install.resources_path))
        .expect("应生成 dynamic/zh-CN.json");
    assert_eq!(dynamic.bytes, br#"{"en":"US"}"#);
    assert!(plan.stats.dynamic_catalog);
}

#[test]
fn collect_install_changes_skips_dynamic_when_en_us_missing() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let pack = fake_pack(root.path(), SAMPLE_REPLACEMENTS);
    let plan = collect_install_changes(&install, &pack, &[], &JobLog::none()).unwrap();
    assert!(!plan
        .changes
        .iter()
        .any(|change| change.target == dynamic_zh_cn_path(&install.resources_path)));
    assert!(!plan.stats.dynamic_catalog);
}

#[test]
fn collect_install_changes_patches_all_whitelists_and_keeps_icons() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let pack = fake_pack(root.path(), SAMPLE_REPLACEMENTS);
    let plan = collect_install_changes(&install, &pack, &[], &JobLog::none()).unwrap();
    let js = plan
        .changes
        .iter()
        .find(|change| change.target == bundle_path(&install))
        .expect("bundle.js 应被改写");
    let patched = String::from_utf8(js.bytes.clone()).unwrap();
    assert_eq!(count_language_lists(&patched), (2, 2));
    assert!(patched.contains(r#"icon:"Add""#));
    assert!(patched.contains(r#"icon:Re?"Check":"Copy""#));
    assert!(patched.contains(r#""tokens"in e&&e.tokens"#));
    assert!(patched.contains(r#"label:"添加""#));
    assert!(patched.contains(r#"name:"简体中文""#));
    assert_eq!(plan.stats.patched_js_count, 1);
    assert_eq!(plan.stats.lists_total, 2);
    assert_eq!(plan.stats.lists_registered, 2);
}

#[test]
fn collect_install_changes_fails_when_no_whitelist_found() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), r#"const a={label:"Add"};"#).unwrap();
    let pack = fake_pack(root.path(), SAMPLE_REPLACEMENTS);
    let error = collect_install_changes(&install, &pack, &[], &JobLog::none()).unwrap_err();
    assert!(error.to_string().contains("未能注册 zh-CN"));
}

// ---------- 步骤 3：备份 / 恢复 / 干净备份 ----------

#[test]
fn validate_restore_target_allows_dynamic_catalog() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    validate_restore_target(&dynamic_zh_cn_path(&install.resources_path), &install, &[]).unwrap();
    assert!(validate_restore_target(
        &dynamic_en_us_path(&install.resources_path),
        &install,
        &[]
    )
    .is_err());
}

#[test]
fn restore_removes_dynamic_catalog_created_by_install() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let dynamic = dynamic_zh_cn_path(&install.resources_path);
    let changes = vec![FileChange {
        target: dynamic.clone(),
        bytes: b"{}".to_vec(),
    }];
    let backup = create_backup(&root.path().join("backups"), &install, &changes).unwrap();
    atomic_write(&dynamic, b"{}").unwrap();
    restore_backup_dir(&backup, &install, &[]).unwrap();
    assert!(!dynamic.exists());
}

#[test]
fn restore_removes_orphan_dynamic_catalog_for_legacy_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let changes = vec![FileChange {
        target: bundle_path(&install),
        bytes: b"patched".to_vec(),
    }];
    // 旧版逻辑的备份清单里没有 dynamic 条目。
    let backup = create_backup(&root.path().join("backups"), &install, &changes).unwrap();
    let dynamic = dynamic_zh_cn_path(&install.resources_path);
    atomic_write(&dynamic, b"{}").unwrap();
    restore_backup_dir(&backup, &install, &[]).unwrap();
    assert!(!dynamic.exists());
}

#[test]
fn restore_latest_backup_resets_zh_locale_even_when_backup_had_zh_locale() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let cfg = locale_path(root.path());
    atomic_write(&cfg, br#"{"locale":"zh-CN","darkMode":"dark"}"#).unwrap();
    let pack = fake_pack(root.path(), SAMPLE_REPLACEMENTS);
    let backups = root.path().join("backups");
    let locales = vec![cfg.clone()];

    install_once(&install, &pack, &backups, &locales);
    restore_latest_backup(&backups, &install, &locales).unwrap();

    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), pristine_js());
    for zh in zh_resource_files(&install) {
        assert!(!zh.exists(), "应删除 {}", zh.display());
    }
    let restored_cfg: Value = read_json_file(&cfg).unwrap().unwrap();
    assert_eq!(restored_cfg["locale"], "en-US");
    assert_eq!(restored_cfg["darkMode"], "dark");
}

#[test]
fn restore_latest_backup_cleans_up_without_same_version_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let (patched_js, _) = register_language(&pristine_js());
    fs::write(bundle_path(&install), &patched_js).unwrap();
    let zh = zh_resource_files(&install);
    atomic_write(&zh[0], b"{}").unwrap();
    let cfg = locale_path(root.path());
    atomic_write(&cfg, br#"{"locale":"zh-CN"}"#).unwrap();
    let backups = root.path().join("backups");
    let locales = vec![cfg.clone()];

    let changed = restore_latest_backup(&backups, &install, &locales).unwrap();
    assert!(changed >= 3);
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), pristine_js());
    assert!(!zh[0].exists());
    let restored_cfg: Value = read_json_file(&cfg).unwrap().unwrap();
    assert_eq!(restored_cfg["locale"], "en-US");
}

fn make_backup_of(install: &LocalizationInstall, backup_root: &Path, js: &str) -> PathBuf {
    fs::write(bundle_path(install), js).unwrap();
    let changes = vec![FileChange {
        target: bundle_path(install),
        bytes: b"new".to_vec(),
    }];
    let backup = create_backup(backup_root, install, &changes).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    backup
}

#[test]
fn clean_backup_selection_skips_patched_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    let clean = make_backup_of(&install, &backups, &pristine_js());
    let (patched_js, _) = register_language(&pristine_js());
    let _patched = make_backup_of(&install, &backups, &patched_js);
    let found = latest_clean_backup_dir(&backups, &install, &[]).unwrap();
    assert_eq!(found, Some(clean));
}

#[test]
fn clean_backup_rejects_tampered_hash() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    let backup = make_backup_of(&install, &backups, &pristine_js());
    fs::write(backup.join("0000.bin"), b"tampered").unwrap();
    assert_eq!(latest_clean_backup_dir(&backups, &install, &[]).unwrap(), None);
}

#[test]
fn clean_backup_rejects_other_claude_version_or_path() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    make_backup_of(&install, &backups, &pristine_js());
    let mut other_version = install.clone();
    other_version.version = "9.9.9".to_string();
    assert_eq!(
        latest_clean_backup_dir(&backups, &other_version, &[]).unwrap(),
        None
    );
    // 同版本目录但安装路径不同：清单校验失败。
    let mut other_path = install.clone();
    other_path.app_path = root.path().join("Elsewhere");
    assert_eq!(
        latest_clean_backup_dir(&backups, &other_path, &[]).unwrap(),
        None
    );
}

#[test]
fn prune_backups_keeps_newest_clean_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    let clean = make_backup_of(&install, &backups, &pristine_js());
    let (patched_js, _) = register_language(&pristine_js());
    for _ in 0..4 {
        make_backup_of(&install, &backups, &patched_js);
    }
    prune_backups(&backups, &install, &[]).unwrap();
    let remaining = fs::read_dir(backups.join("1.2.3")).unwrap().count();
    assert_eq!(remaining, BACKUPS_TO_KEEP + 1);
    assert!(clean.is_dir());
}

// ---------- 步骤 4：基线 + 安装流程 ----------

/// 第一次安装一棵原版假树，留下一份干净备份，返回安装后的 bundle 内容。
fn install_once(
    install: &LocalizationInstall,
    pack: &ValidatedPack,
    backups: &Path,
    locales: &[PathBuf],
) -> String {
    perform_install_core(install, pack, backups, locales, None, &JobLog::none()).unwrap();
    fs::read_to_string(bundle_path(install)).unwrap()
}

#[test]
fn baseline_noop_on_pristine_tree() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let (outcome, scan) = prepare_clean_baseline(
        &install,
        &[],
        &root.path().join("backups"),
        None,
        &JobLog::none(),
    )
    .unwrap();
    assert_eq!(outcome, BaselineOutcome::AlreadyClean);
    assert_eq!(scan.lists_with_zh, 0);
}

#[test]
fn baseline_restores_original_from_newest_clean_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let zh = zh_resource_files(&install);
    let changes = vec![
        FileChange {
            target: bundle_path(&install),
            bytes: b"x".to_vec(),
        },
        FileChange {
            target: zh[0].clone(),
            bytes: b"{}".to_vec(),
        },
        FileChange {
            target: zh[3].clone(),
            bytes: b"{}".to_vec(),
        },
    ];
    let backup = create_backup(&backups, &install, &changes).unwrap();
    // 模拟旧逻辑把磁盘改坏。
    fs::write(bundle_path(&install), corrupted_js()).unwrap();
    atomic_write(&zh[0], b"{}").unwrap();
    atomic_write(&zh[3], b"{}").unwrap();

    let (outcome, scan) =
        prepare_clean_baseline(&install, &[], &backups, None, &JobLog::none()).unwrap();

    assert_eq!(outcome, BaselineOutcome::RestoredFromBackup(backup));
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), pristine_js());
    assert!(!zh[0].exists());
    assert!(!zh[3].exists());
    assert_eq!(scan.lists_with_zh, 0);
}

#[test]
fn baseline_fails_with_reinstall_message_when_corrupted_and_no_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), corrupted_js()).unwrap();
    let error = prepare_clean_baseline(
        &install,
        &[],
        &root.path().join("backups"),
        None,
        &JobLog::none(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("重新安装 Claude Desktop"));
    // 失败时不改动磁盘。
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), corrupted_js());
}

#[test]
fn baseline_fails_when_only_corruption_markers_and_no_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), markers_only_corrupted_js()).unwrap();
    let error = prepare_clean_baseline(
        &install,
        &[],
        &root.path().join("backups"),
        None,
        &JobLog::none(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("没有可用的原版备份"));
    // 失败时不改动磁盘。
    assert_eq!(
        fs::read_to_string(bundle_path(&install)).unwrap(),
        markers_only_corrupted_js()
    );
}

#[test]
fn baseline_restores_original_when_only_corruption_markers_and_clean_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    make_backup_of(&install, &backups, &pristine_js());
    fs::write(bundle_path(&install), markers_only_corrupted_js()).unwrap();
    let (outcome, scan) =
        prepare_clean_baseline(&install, &[], &backups, None, &JobLog::none()).unwrap();
    assert!(matches!(outcome, BaselineOutcome::RestoredFromBackup(_)));
    assert!(!scan.is_corrupted());
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), pristine_js());
}

#[test]
fn baseline_ignores_backup_for_other_claude_version() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    let mut old = install.clone();
    old.version = "0.0.1".to_string();
    make_backup_of(&old, &backups, &pristine_js());
    fs::write(bundle_path(&install), corrupted_js()).unwrap();
    let result = prepare_clean_baseline(&install, &[], &backups, None, &JobLog::none());
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), corrupted_js());
}

#[test]
fn baseline_continues_when_patched_but_clean_and_no_backup() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let (patched, _) = register_language(&pristine_js());
    fs::write(bundle_path(&install), &patched).unwrap();
    let (outcome, _) = prepare_clean_baseline(
        &install,
        &[],
        &root.path().join("backups"),
        None,
        &JobLog::none(),
    )
    .unwrap();
    assert_eq!(outcome, BaselineOutcome::PatchedButClean);
}

#[test]
fn baseline_refuses_dirty_backup_and_leaves_disk_untouched() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    let backups = root.path().join("backups");
    // 备份本身就是被污染的内容：不能当作基线。
    make_backup_of(&install, &backups, &corrupted_js());
    fs::write(bundle_path(&install), corrupted_js()).unwrap();
    let error =
        prepare_clean_baseline(&install, &[], &backups, None, &JobLog::none()).unwrap_err();
    assert!(error.to_string().contains("没有可用的原版备份"));
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), corrupted_js());
}

#[test]
fn install_flow_end_to_end_on_fake_tree() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    write_dynamic_en_us(&install);
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let pack = fake_pack(root.path(), SAMPLE_REPLACEMENTS);
    let backups = root.path().join("backups");
    let locales = vec![locale_path(root.path())];

    let first = install_once(&install, &pack, &backups, &locales);
    // 旧逻辑把磁盘改坏（保留已写入的 zh-CN 资源文件）。
    fs::write(bundle_path(&install), corrupted_js()).unwrap();

    let outcome =
        perform_install_core(&install, &pack, &backups, &locales, None, &JobLog::none()).unwrap();
    assert!(matches!(
        outcome.baseline,
        Some(BaselineOutcome::RestoredFromBackup(_))
    ));
    let second = fs::read_to_string(bundle_path(&install)).unwrap();
    assert_eq!(second, first, "从原版重装的结果应与首次安装一致");
    assert!(!second.contains("icon:\"添"));
    assert!(!second.contains("\"令牌\"in"));
    assert_eq!(count_language_lists(&second), (2, 2));
    assert!(dynamic_zh_cn_path(&install.resources_path).is_file());
    let locale: Value = read_json_file(&locales[0]).unwrap().unwrap();
    assert_eq!(locale["locale"], "zh-CN");

    // 第三次：幂等，字节完全相同。
    perform_install_core(&install, &pack, &backups, &locales, None, &JobLog::none()).unwrap();
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), second);
}

#[test]
fn apply_changes_rolls_back_when_post_verify_fails() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let created = zh_resource_files(&install)[0].clone();
    let changes = vec![
        FileChange {
            target: bundle_path(&install),
            bytes: b"patched".to_vec(),
        },
        FileChange {
            target: created.clone(),
            bytes: b"{}".to_vec(),
        },
    ];
    let backup = create_backup(&root.path().join("backups"), &install, &changes).unwrap();
    let error = apply_changes_with_rollback(&changes, &backup, &install, &[], || {
        Err(AppError::Config("故意失败".to_string()))
    })
    .unwrap_err();
    assert!(error.to_string().contains("已自动回滚"));
    assert_eq!(fs::read_to_string(bundle_path(&install)).unwrap(), pristine_js());
    assert!(!created.exists());
}

#[test]
fn corruption_self_check_triggers_rollback() {
    let root = tempdir().unwrap();
    let install = fake_install(root.path());
    fs::write(bundle_path(&install), pristine_js()).unwrap();
    let pack = fake_pack(root.path(), SAMPLE_REPLACEMENTS);
    let locales = vec![locale_path(root.path())];
    let baseline = scan_dir(&frontend_assets_dir(&install.resources_path)).unwrap();
    let mut plan = collect_install_changes(&install, &pack, &locales, &JobLog::none()).unwrap();
    // 模拟补丁引擎出错：注册了白名单，但图标名被翻成了中文。
    let bundle = bundle_path(&install);
    let corrupted = {
        let (registered, _) = register_language(&pristine_js());
        registered.replace("icon:\"Add\"", "icon:\"添加\"")
    };
    for change in &mut plan.changes {
        if change.target == bundle {
            change.bytes = corrupted.clone().into_bytes();
        }
    }
    let backup = create_backup(&root.path().join("backups"), &install, &plan.changes).unwrap();

    let error = apply_changes_with_rollback(&plan.changes, &backup, &install, &locales, || {
        post_verify_installed(&install, &locales, &plan.stats, &baseline)
    })
    .unwrap_err();

    assert!(error.to_string().contains("已自动回滚"), "{error}");
    assert!(error.to_string().contains("图标名"), "{error}");
    assert_eq!(fs::read_to_string(&bundle).unwrap(), pristine_js());
    assert!(!zh_resource_files(&install)[0].exists());
    assert!(!locales[0].exists());
}

#[test]
fn last_install_info_roundtrip() {
    let root = tempdir().unwrap();
    let info = LastInstallInfo {
        claude_version: "1.2.3".to_string(),
        install_path: "C:/x".to_string(),
        pack_version: Some("9.9.9".to_string()),
        patched_js_count: 7,
        installed_at: 123,
        log_path: "C:/log".to_string(),
    };
    assert_eq!(read_last_install(root.path()), None);
    write_last_install(root.path(), &info).unwrap();
    assert_eq!(read_last_install(root.path()), Some(info));
    clear_last_install(root.path());
    assert_eq!(read_last_install(root.path()), None);
}

// ---------- 步骤 5：进程 / 重启（纯函数） ----------

#[test]
fn parse_pid_list_ignores_blank_and_garbage_lines() {
    assert_eq!(parse_pid_list("123\r\n\r\nabc\n 456 \n0\n-7\n"), vec![123, 456]);
    assert!(parse_pid_list("").is_empty());
}

#[test]
fn appx_package_family_is_derived_from_install_dir_name() {
    assert_eq!(
        appx_package_family(Path::new(r"C:\Program Files\WindowsApps\Claude_2.0.0.0_x64__abc123xyz")),
        Some("Claude_abc123xyz".to_string())
    );
    assert_eq!(appx_package_family(Path::new("Claude_1.0")), None);
    assert_eq!(appx_package_family(Path::new("Claude_1.0_x64__bad id")), None);
}

#[test]
fn select_appx_app_id_prefers_family_match_and_rejects_unsafe_ids() {
    let lines = vec![
        "Microsoft.Edge".to_string(),
        "Claude_other!App".to_string(),
        "Claude_abc123xyz!Claude".to_string(),
        "Claude_abc123xyz!x; calc".to_string(),
    ];
    assert_eq!(
        select_appx_app_id(&lines, Some("Claude_abc123xyz")),
        Some("Claude_abc123xyz!Claude".to_string())
    );
    // 包族找不到时退到 Claude_*!*。
    assert_eq!(
        select_appx_app_id(&lines, Some("Claude_missing")),
        Some("Claude_other!App".to_string())
    );
    let unsafe_only = vec!["Claude_abc!x; calc".to_string(), "Claude_abc!x y".to_string()];
    assert_eq!(select_appx_app_id(&unsafe_only, None), None);
}

#[test]
fn appx_launch_argument_uses_apps_folder_and_rejects_unsafe_ids() {
    assert_eq!(
        appx_launch_argument("Claude_abc!Claude"),
        Some("shell:AppsFolder\\Claude_abc!Claude".to_string())
    );
    assert_eq!(appx_launch_argument("a b"), None);
    assert_eq!(appx_launch_argument("a&calc"), None);
    assert_eq!(appx_launch_argument(""), None);
}

fn ok_result(message: &str) -> DesktopLocalizationActionResult {
    DesktopLocalizationActionResult::simple(true, 1, message.to_string(), None)
}

#[test]
fn finish_action_does_not_restart_when_worker_failed() {
    let mut called = false;
    let error = finish_action(Err(AppError::Config("worker 失败".to_string())), "install", || {
        called = true;
        RestartOutcome::ok("appx", "")
    })
    .unwrap_err();
    assert!(!called, "worker 失败时不得重启");
    assert!(error.to_string().contains("worker 失败"));
}

#[test]
fn finish_action_install_message_asks_to_confirm_language() {
    let result = finish_action(Ok(ok_result("安装完成")), "install", || {
        RestartOutcome::ok("appx", "已启动 Claude Desktop")
    })
    .unwrap();
    assert!(result.message.contains("已重启 Claude Desktop"));
    assert!(result.message.contains("Language"));
    assert!(result.message.contains("简体中文"));
    assert_eq!(result.restart.as_deref(), Some("restarted"));
}

#[test]
fn finish_action_surfaces_restart_failure_in_message() {
    let result = finish_action(Ok(ok_result("安装完成")), "install", || {
        RestartOutcome::failed("appx", "开始菜单里没有找到 Claude 的应用入口")
    })
    .unwrap();
    assert!(result.ok);
    assert!(result.message.contains("自动重启失败"));
    assert!(result.message.contains("开始菜单里没有找到"));
    assert_eq!(result.restart.as_deref(), Some("failed"));
    assert_eq!(
        result.restart_detail.as_deref(),
        Some("开始菜单里没有找到 Claude 的应用入口")
    );
}

#[test]
fn finish_action_restore_message_has_no_language_hint() {
    let result = finish_action(Ok(ok_result("已恢复")), "restore", || {
        RestartOutcome::ok("exe", "已启动 Claude Desktop")
    })
    .unwrap();
    assert!(!result.message.contains("Language"));
}

// ---------- 步骤 6：状态判定 ----------

fn full_scan() -> PatchScan {
    PatchScan {
        js_files: 3,
        lists_total: 2,
        lists_with_zh: 2,
        icon_cjk: 0,
        token_marker: 0,
        skipped_non_utf8: 0,
    }
}

fn ready_inputs() -> StateInputs {
    StateInputs {
        frontend: true,
        desktop: true,
        statsig: true,
        dynamic_required: true,
        dynamic_present: true,
        locale_is_zh: true,
        scan: Some(full_scan()),
        scan_error: None,
        last_install: None,
        claude_version: "1.2.3".to_string(),
    }
}

#[test]
fn evaluate_state_installed_requires_every_condition() {
    assert_eq!(evaluate_install_state(&ready_inputs()), ("installed", Vec::new()));

    let mut missing_file = ready_inputs();
    missing_file.statsig = false;
    let (state, reasons) = evaluate_install_state(&missing_file);
    assert_eq!(state, "partial");
    assert!(reasons[0].contains("缺少资源文件"));

    let mut wrong_locale = ready_inputs();
    wrong_locale.locale_is_zh = false;
    let (state, reasons) = evaluate_install_state(&wrong_locale);
    assert_eq!(state, "partial");
    assert!(reasons.iter().any(|reason| reason.contains("locale")));

    let mut no_scan = ready_inputs();
    no_scan.scan = None;
    no_scan.scan_error = Some("目录缺失".to_string());
    let (state, reasons) = evaluate_install_state(&no_scan);
    assert_eq!(state, "partial");
    assert!(reasons.iter().any(|reason| reason.contains("目录缺失")));
}

#[test]
fn evaluate_state_partial_when_dynamic_catalog_missing() {
    let mut inputs = ready_inputs();
    inputs.dynamic_present = false;
    let (state, reasons) = evaluate_install_state(&inputs);
    assert_eq!(state, "partial");
    assert!(reasons.iter().any(|reason| reason.contains("dynamic/zh-CN.json")));

    // Claude 没有 dynamic/en-US.json 时不要求它。
    inputs.dynamic_required = false;
    assert_eq!(evaluate_install_state(&inputs).0, "installed");
}

#[test]
fn evaluate_state_partial_when_a_whitelist_lacks_zh_cn() {
    let mut inputs = ready_inputs();
    inputs.scan = Some(PatchScan {
        lists_with_zh: 1,
        ..full_scan()
    });
    let (state, reasons) = evaluate_install_state(&inputs);
    assert_eq!(state, "partial");
    assert!(reasons.iter().any(|reason| reason.contains("1/2")));
}

#[test]
fn evaluate_state_partial_when_corruption_markers_present() {
    let mut inputs = ready_inputs();
    inputs.scan = Some(PatchScan {
        icon_cjk: 5,
        token_marker: 2,
        ..full_scan()
    });
    let (state, reasons) = evaluate_install_state(&inputs);
    assert_eq!(state, "partial");
    assert!(reasons.iter().any(|reason| reason.contains("损坏内容")));
}

#[test]
fn evaluate_state_partial_when_claude_version_changed() {
    let mut inputs = ready_inputs();
    inputs.last_install = Some(LastInstallInfo {
        claude_version: "1.0.0".to_string(),
        install_path: String::new(),
        pack_version: None,
        patched_js_count: 1,
        installed_at: 1,
        log_path: String::new(),
    });
    let (state, reasons) = evaluate_install_state(&inputs);
    assert_eq!(state, "partial");
    assert!(reasons.iter().any(|reason| reason.contains("1.0.0") && reason.contains("1.2.3")));
}

#[test]
fn evaluate_state_not_installed_when_everything_absent() {
    let inputs = StateInputs {
        frontend: false,
        desktop: false,
        statsig: false,
        dynamic_required: true,
        dynamic_present: false,
        locale_is_zh: false,
        scan: None,
        scan_error: None,
        last_install: None,
        claude_version: "1.2.3".to_string(),
    };
    assert_eq!(evaluate_install_state(&inputs), ("notInstalled", Vec::new()));
}

#[test]
fn evaluate_state_partial_when_claude_updated_but_locale_still_zh() {
    let mut inputs = ready_inputs();
    inputs.frontend = false;
    inputs.desktop = false;
    inputs.statsig = false;
    inputs.dynamic_present = false;
    inputs.scan = None;
    assert_eq!(evaluate_install_state(&inputs).0, "partial");
}

#[test]
fn scan_cache_reuses_result_until_fingerprint_changes() {
    let mut cache = ScanCache::default();
    let key = |stamp: u64| ScanKey {
        resources_path: PathBuf::from("r"),
        stamps: vec![Some((stamp, None))],
    };
    let mut scans = 0;
    for stamp in [1, 1, 1, 2, 2] {
        cache
            .get_or_scan(key(stamp), || {
                scans += 1;
                Ok(full_scan())
            })
            .unwrap();
    }
    assert_eq!(scans, 2);
}

// ---------- 步骤 9：真实 bundle 副本冒烟（默认忽略，只读源、写临时副本） ----------

/// `AISW_LOC_BUNDLE_COPY` 指向 %TEMP% 下从备份拷出的原版 JS 副本目录，
/// `AISW_LOC_HARDCODED` 指向 pack 的 frontend-hardcoded-zh-CN.json（只读）。
#[test]
#[ignore = "需要手动准备的真实 bundle 副本"]
fn real_bundle_copy_patch_smoke() {
    let (Some(copy), Some(hardcoded)) = (
        std::env::var_os("AISW_LOC_BUNDLE_COPY").map(PathBuf::from),
        std::env::var_os("AISW_LOC_HARDCODED").map(PathBuf::from),
    ) else {
        eprintln!("未设置 AISW_LOC_BUNDLE_COPY / AISW_LOC_HARDCODED，跳过");
        return;
    };
    let baseline = scan_dir(&copy).unwrap();
    assert!(!baseline.is_corrupted(), "原版副本不应被判为损坏: {baseline:?}");
    assert_eq!(baseline.lists_with_zh, 0, "原版副本不应含 zh-CN: {baseline:?}");
    let rules = load_hardcoded_replacements(&hardcoded).unwrap();
    let mut patched_total = 0;
    let mut result = PatchScan::default();
    for entry in fs::read_dir(&copy).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("js") {
            continue;
        }
        let original = fs::read_to_string(&path).unwrap();
        let (mut patched, _) = register_language(&original);
        apply_hardcoded_replacements(&mut patched, &rules);
        verify_patched_sources(&original, &patched)
            .unwrap_or_else(|reason| panic!("{}: {reason}", path.display()));
        if patched != original {
            patched_total += 1;
        }
        result.add_text(&patched);
    }
    eprintln!("baseline={baseline:?} patched_files={patched_total} result={result:?}");
    assert!(result.all_lists_registered(), "{result:?}");
    assert_eq!(result.icon_cjk, 0);
    assert_eq!(result.token_marker, 0);
}
