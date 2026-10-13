// 安装/恢复流程：日志、安装记录、基线准备、写入 + 回滚 + 写入后校验。
// 由 desktop_localization.rs `include!`。

const LAST_INSTALL_FILE: &str = "desktop-last-install.json";

/// worker 逐步追加的过程日志（写失败只告警，不影响主流程）。
#[derive(Debug, Clone, Default)]
struct JobLog {
    path: Option<PathBuf>,
}

impl JobLog {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// 不落盘的日志（测试和纯函数路径用）。
    fn none() -> Self {
        Self { path: None }
    }

    fn line(&self, message: &str) {
        let Some(path) = &self.path else {
            return;
        };
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
            write!(file, "{} {message}\r\n", Utc::now().to_rfc3339())
        };
        if let Err(error) = write() {
            log::warn!("写入中文化日志失败 {}: {error}", path.display());
        }
    }
}

/// 最近一次成功安装的记录。只用于展示和识别「Claude 版本已变」，不参与 installed 判定。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LastInstallInfo {
    claude_version: String,
    install_path: String,
    pack_version: Option<String>,
    patched_js_count: usize,
    installed_at: i64,
    log_path: String,
}

fn last_install_dir() -> PathBuf {
    get_app_config_dir().join("localization")
}

fn read_last_install(dir: &Path) -> Option<LastInstallInfo> {
    read_json_file::<LastInstallInfo>(&dir.join(LAST_INSTALL_FILE))
        .ok()
        .flatten()
}

fn write_last_install(dir: &Path, info: &LastInstallInfo) -> AppResult<()> {
    fs::create_dir_all(dir)?;
    write_json_file(&dir.join(LAST_INSTALL_FILE), info)
}

fn clear_last_install(dir: &Path) {
    let _ = fs::remove_file(dir.join(LAST_INSTALL_FILE));
}

/// worker 执行结果（给消息和安装记录用）。
#[derive(Debug, Clone, Default)]
struct WorkerOutcome {
    changed_files: usize,
    patched_js_count: usize,
    lists_registered: usize,
    baseline: Option<BaselineOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BaselineOutcome {
    /// 磁盘上没有我们的痕迹。
    AlreadyClean,
    /// 从同版本的干净备份还原了原版文件。
    RestoredFromBackup(PathBuf),
    /// 有我们的痕迹，但没有腐败标记，也没有可用备份：在已补丁内容上继续（补丁幂等）。
    PatchedButClean,
}

/// 我们会写入的 4 个 zh-CN 资源文件（3 个固定 + dynamic catalog）。
fn zh_resource_files(install: &LocalizationInstall) -> [PathBuf; 4] {
    [
        install
            .resources_path
            .join("ion-dist")
            .join("i18n")
            .join("zh-CN.json"),
        install.resources_path.join("zh-CN.json"),
        install
            .resources_path
            .join("ion-dist")
            .join("i18n")
            .join("statsig")
            .join("zh-CN.json"),
        dynamic_zh_cn_path(&install.resources_path),
    ]
}

/// 备份是否「干净」：同版本同路径、哈希对得上、不含我们的补丁痕迹和腐败标记。
fn backup_is_clean(
    backup_dir: &Path,
    manifest: &LocalizationBackupManifest,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
) -> bool {
    if validate_backup_manifest(manifest, install).is_err() {
        return false;
    }
    let zh_files = zh_resource_files(install);
    let mut scan = PatchScan::default();
    for file in &manifest.files {
        if validate_restore_target(&file.target, install, locale_paths).is_err() {
            return false;
        }
        if file.existed && zh_files.contains(&file.target) {
            return false;
        }
        if !file.existed {
            continue;
        }
        let Some(name) = file.backup_file.as_deref() else {
            return false;
        };
        if name.is_empty() || name.contains(['/', '\\']) || name == ".." {
            return false;
        }
        let Ok(bytes) = fs::read(backup_dir.join(name)) else {
            return false;
        };
        if file.sha256.as_deref() != Some(hex::encode(Sha256::digest(&bytes)).as_str()) {
            return false;
        }
        if file.target.extension().and_then(|ext| ext.to_str()) == Some("js") {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                scan.add_text(text);
            }
        }
    }
    scan.lists_with_zh == 0 && !scan.is_corrupted()
}

/// 同版本里最新的一份干净备份（按时间戳目录名倒序）。
fn latest_clean_backup_dir(
    backup_root: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
) -> AppResult<Option<PathBuf>> {
    let version_dir = backup_root.join(safe_component(&install.version));
    if !version_dir.is_dir() {
        return Ok(None);
    }
    let mut candidates = fs::read_dir(version_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("manifest.json").is_file())
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    for candidate in candidates {
        let Ok(bytes) = fs::read(candidate.join("manifest.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<LocalizationBackupManifest>(&bytes) else {
            continue;
        };
        if backup_is_clean(&candidate, &manifest, install, locale_paths) {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// 恢复完成后，清掉旧版本逻辑装过、但备份清单里没记录的 dynamic/zh-CN.json。
fn restore_orphan_dynamic_catalog(
    install: &LocalizationInstall,
    manifest: &LocalizationBackupManifest,
    locale_paths: &[PathBuf],
) -> AppResult<()> {
    let dynamic = dynamic_zh_cn_path(&install.resources_path);
    if manifest.files.iter().any(|file| file.target == dynamic) || !dynamic.exists() {
        return Ok(());
    }
    validate_restore_target(&dynamic, install, locale_paths)?;
    fs::remove_file(&dynamic)?;
    Ok(())
}

/// 安装前把磁盘还原到「Claude 原版」。绝不在腐败内容上继续打补丁。
fn prepare_clean_baseline(
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
    backup_root: &Path,
    previous: Option<&LastInstallInfo>,
    log: &JobLog,
) -> AppResult<(BaselineOutcome, PatchScan)> {
    if let Some(previous) = previous {
        if previous.claude_version != install.version {
            log.line(&format!(
                "检测到 Claude 版本变化：{} → {}，将在新版本目录上重新安装",
                previous.claude_version, install.version
            ));
        }
    }
    let assets = frontend_assets_dir(&install.resources_path);
    let scan = scan_dir(&assets)?;
    let leftovers = zh_resource_files(install)
        .iter()
        .filter(|path| path.exists())
        .count();
    log.line(&format!(
        "基线扫描：{} 个前端文件，白名单 {}/{} 含 zh-CN，图标名被翻译 {} 处，逻辑词被替换 {} 处，zh-CN 资源文件 {} 个",
        scan.js_files,
        scan.lists_with_zh,
        scan.lists_total,
        scan.icon_cjk,
        scan.token_marker,
        leftovers
    ));
    // 腐败标记本身也算「我们的痕迹」：即使白名单未注册、没有 zh 资源文件，
    // 也不能在损坏内容上继续打补丁。
    if leftovers == 0 && scan.lists_with_zh == 0 && !scan.is_corrupted() {
        return Ok((BaselineOutcome::AlreadyClean, scan));
    }
    match latest_clean_backup_dir(backup_root, install, locale_paths)? {
        Some(backup_dir) => {
            log.line(&format!("从备份恢复原版：{}", backup_dir.display()));
            restore_backup_dir(&backup_dir, install, locale_paths)?;
            let restored = scan_dir(&assets)?;
            let leftovers_after = zh_resource_files(install)
                .iter()
                .filter(|path| path.exists())
                .count();
            if restored.lists_with_zh > 0 || restored.is_corrupted() || leftovers_after > 0 {
                return Err(AppError::Config(
                    "无法还原到 Claude 原版内容（备份恢复后仍检测到中文化痕迹）。请通过应用商店修复或重新安装 Claude Desktop，或先点「恢复原版」后重试".to_string(),
                ));
            }
            Ok((BaselineOutcome::RestoredFromBackup(backup_dir), restored))
        }
        None if scan.is_corrupted() => Err(AppError::Config(
            "检测到旧版中文化遗留的损坏内容，且没有可用的原版备份。请通过应用商店「修复/重置」或重新安装 Claude Desktop 后再安装中文化".to_string(),
        )),
        None => {
            log.line("检测到已有中文化内容但无干净备份，内容未损坏，直接在其上幂等补丁");
            Ok((BaselineOutcome::PatchedButClean, scan))
        }
    }
}

/// 写入全部修改；任一步失败（含写入后校验）都回滚到备份。
fn apply_changes_with_rollback(
    changes: &[FileChange],
    backup_dir: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
    post_verify: impl FnOnce() -> AppResult<()>,
) -> AppResult<()> {
    let mut applied_targets = HashSet::new();
    for change in changes {
        if let Err(error) = atomic_write(&change.target, &change.bytes) {
            let install_error = format!(
                "写入中文化文件失败 {}: {error}",
                change.target.display()
            );
            if applied_targets.is_empty() {
                return Err(AppError::Other(format!(
                    "安装失败，尚未写入任何文件：{install_error}"
                )));
            }
            let rollback_error =
                restore_backup_targets(backup_dir, install, locale_paths, &applied_targets).err();
            return Err(AppError::Other(match rollback_error {
                Some(rollback) => {
                    format!("安装失败且自动回滚失败：{install_error}；{rollback}")
                }
                None => format!("安装失败，已自动回滚：{install_error}"),
            }));
        }
        applied_targets.insert(change.target.clone());
    }
    if let Err(error) = post_verify() {
        let rollback_error =
            restore_backup_targets(backup_dir, install, locale_paths, &applied_targets).err();
        return Err(AppError::Other(match rollback_error {
            Some(rollback) => {
                format!("安装后校验未通过且自动回滚失败：{error}；{rollback}")
            }
            None => format!("安装后校验未通过，已自动回滚：{error}"),
        }));
    }
    Ok(())
}

/// 写入后读磁盘断言：资源文件、dynamic、locale、白名单和腐败标记。
fn post_verify_installed(
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
    stats: &InstallStats,
    baseline: &PatchScan,
) -> AppResult<()> {
    let zh_files = zh_resource_files(install);
    for (index, path) in zh_files.iter().enumerate() {
        if index == 3 && !stats.dynamic_catalog {
            continue;
        }
        let non_empty = fs::metadata(path).map(|meta| meta.len() > 0).unwrap_or(false);
        if !non_empty {
            return Err(AppError::Config(format!(
                "写入后缺少或为空的资源文件: {}",
                path.display()
            )));
        }
    }
    for path in locale_paths {
        let locale = read_json_file::<Value>(path)
            .ok()
            .flatten()
            .and_then(|value| value.get("locale")?.as_str().map(str::to_string));
        if locale.as_deref() != Some(LOCALIZATION_LOCALE) {
            return Err(AppError::Config(format!(
                "写入后 locale 不是 zh-CN: {}",
                path.display()
            )));
        }
    }
    let scan = scan_dir(&frontend_assets_dir(&install.resources_path))?;
    if !scan.all_lists_registered() {
        return Err(AppError::Config(format!(
            "写入后仍有语言白名单未注册 zh-CN（{}/{}）",
            scan.lists_with_zh, scan.lists_total
        )));
    }
    if scan.icon_cjk > baseline.icon_cjk {
        return Err(AppError::Config(format!(
            "写入后检测到图标名被翻译成中文（{} 处）",
            scan.icon_cjk
        )));
    }
    if scan.token_marker > baseline.token_marker {
        return Err(AppError::Config(format!(
            "写入后检测到逻辑词被翻译（{} 处）",
            scan.token_marker
        )));
    }
    Ok(())
}

fn perform_worker_action(
    job: &LocalizationWorkerJob,
    log: &JobLog,
) -> AppResult<WorkerOutcome> {
    let stopped = stop_claude_verified(&job.install, log)?;
    let result = (|| {
        if job.install.kind == "appx" {
            log.line("取得 Claude 资源目录写入权限");
            ensure_write_access(&job.install.resources_path)?;
        }
        if job.action == "restore" {
            restore_latest_backup(&job.backup_root, &job.install, &job.locale_paths).map(
                |changed_files| WorkerOutcome {
                    changed_files,
                    ..WorkerOutcome::default()
                },
            )
        } else if job.action == "install" {
            perform_install(job, log)
        } else {
            Err(AppError::Config("未知的中文化操作".to_string()))
        }
    })();
    result.map_err(|error| {
        if stopped {
            AppError::Other(format!(
                "{error}。Claude Desktop 已被关闭，请从开始菜单重新打开"
            ))
        } else {
            error
        }
    })
}

fn perform_install(job: &LocalizationWorkerJob, log: &JobLog) -> AppResult<WorkerOutcome> {
    let pack_path = job
        .pack_path
        .as_ref()
        .ok_or_else(|| AppError::Config("中文资源包路径缺失".to_string()))?;
    let pack = validate_pack(pack_path)?;
    let previous = read_last_install(&last_install_dir());
    perform_install_core(
        &job.install,
        &pack,
        &job.backup_root,
        &job.locale_paths,
        previous.as_ref(),
        log,
    )
}

/// 安装主体（不碰进程和提权，便于在假目录树上测试）。
/// 顺序：基线还原 → 生成计划（失败时磁盘仍是原版）→ 备份 → 写入 → 写入后校验 → 轮换备份。
fn perform_install_core(
    install: &LocalizationInstall,
    pack: &ValidatedPack,
    backup_root: &Path,
    locale_paths: &[PathBuf],
    previous: Option<&LastInstallInfo>,
    log: &JobLog,
) -> AppResult<WorkerOutcome> {
    let (baseline, baseline_scan) =
        prepare_clean_baseline(install, locale_paths, backup_root, previous, log)?;
    let plan = collect_install_changes(install, pack, locale_paths, log)?;
    if plan.changes.is_empty() {
        return Err(AppError::Config(
            "没有生成任何中文化文件修改".to_string(),
        ));
    }
    let backup_dir = create_backup(backup_root, install, &plan.changes)?;
    log.line(&format!("已创建备份：{}", backup_dir.display()));
    apply_changes_with_rollback(&plan.changes, &backup_dir, install, locale_paths, || {
        post_verify_installed(install, locale_paths, &plan.stats, &baseline_scan)
    })?;
    log.line(&format!("已写入 {} 个文件，写入后校验通过", plan.changes.len()));
    if let Err(error) = prune_backups(backup_root, install, locale_paths) {
        log::warn!("轮换中文化备份失败（安装已完成）: {error}");
    }
    Ok(WorkerOutcome {
        changed_files: plan.changes.len(),
        patched_js_count: plan.stats.patched_js_count,
        lists_registered: plan.stats.lists_registered,
        baseline: Some(baseline),
    })
}
