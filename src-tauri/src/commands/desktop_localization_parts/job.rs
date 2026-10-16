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

/// 将用户配置目录中 `locale == "zh-CN"` 的 `config.json` 重置回 `"en-US"`，
/// 避免跨版本升级后备份本身带有 `"zh-CN"` 或无同版本备份时残留 `locale=zh-CN` 导致状态卡在 `partial`。
fn clear_zh_locale_in_configs(locale_paths: &[PathBuf]) -> AppResult<usize> {
    let mut changed = 0usize;
    for path in locale_paths {
        if !path.is_file() {
            continue;
        }
        let Ok(Some(mut value)) = read_json_file::<Value>(path) else {
            continue;
        };
        let Some(object) = value.as_object_mut() else {
            continue;
        };
        if object.get("locale").and_then(Value::as_str) != Some(LOCALIZATION_LOCALE) {
            continue;
        }
        object.insert("locale".to_string(), Value::String("en-US".to_string()));
        write_json_file(path, &value)?;
        changed += 1;
    }
    Ok(changed)
}

/// 从前端 assets 目录所有 `.js` 文件的语言白名单中移除 `zh-CN`。
fn unregister_zh_from_assets(
    assets_dir: &Path,
    install: &LocalizationInstall,
    locale_paths: &[PathBuf],
) -> AppResult<usize> {
    let mut changed_files = 0usize;
    for entry in fs::read_dir(assets_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("js") || !path.is_file() {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let (unregistered, lists_changed) = unregister_language(&text);
        if lists_changed > 0 {
            validate_restore_target(&path, install, locale_paths)?;
            atomic_write(&path, unregistered.as_bytes())?;
            changed_files += 1;
        }
    }
    Ok(changed_files)
}

/// 判断当前卸载/恢复操作是否只需清理用户态配置（`config.json` 与 `desktop-last-install.json`），
/// 而无需触碰受保护的 `WindowsApps` 资源目录（从而免除 UAC 提权弹窗）。
fn can_restore_user_state_without_elevation(
    install: &LocalizationInstall,
    backup_root: &Path,
    locale_paths: &[PathBuf],
) -> bool {
    if latest_clean_backup_dir(backup_root, install, locale_paths)
        .ok()
        .flatten()
        .is_some()
    {
        return false;
    }
    if count_healable_cross_version_corrupted_assets(backup_root, install) > 0 {
        return false;
    }
    if zh_resource_files(install).iter().any(|path| path.exists()) {
        return false;
    }
    let assets_dir = frontend_assets_dir(&install.resources_path);
    match scan_dir(&assets_dir) {
        Ok(scan) => !scan.is_corrupted() && scan.lists_with_zh == 0,
        Err(_) => false,
    }
}

/// 从 MSIX `AppxBlockMap.xml` 中提取 `ion-dist/assets/v1/*.js` 的官方 SHA-256（hex）。
fn parse_appx_blockmap_asset_hashes(app_path: &Path) -> std::collections::HashMap<String, String> {
    use base64::Engine as _;
    let mut out = std::collections::HashMap::new();
    let blockmap_path = app_path.join("AppxBlockMap.xml");
    let Ok(xml) = fs::read_to_string(&blockmap_path) else {
        return out;
    };
    for segment in xml.split("<File ") {
        let Some(name_start) = segment.find("Name=\"") else {
            continue;
        };
        let rest = &segment[name_start + 6..];
        let Some(name_end) = rest.find('"') else {
            continue;
        };
        let rel_name = &rest[..name_end];
        if !rel_name.ends_with(".js") || !rel_name.contains("ion-dist\\assets\\v1\\") {
            continue;
        }
        let Some(file_name) = Path::new(rel_name).file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(fh_pos) = segment.find("FileHash") else {
            continue;
        };
        let fh_rest = &segment[fh_pos..];
        let Some(hash_start) = fh_rest.find("Hash=\"") else {
            continue;
        };
        let hash_rest = &fh_rest[hash_start + 6..];
        let Some(hash_end) = hash_rest.find('"') else {
            continue;
        };
        let b64 = &hash_rest[..hash_end];
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(b64) {
            if decoded.len() == 32 {
                out.insert(file_name.to_string(), hex::encode(decoded));
            }
        }
    }
    out
}

/// 快速检测当前 MSIX 版本是否存在可由历史备份修复的增量更新损坏文件（仅检查历史备份中涉及的 ~450 个文件）。
fn count_healable_cross_version_corrupted_assets(
    backup_root: &Path,
    install: &LocalizationInstall,
) -> usize {
    if !backup_root.is_dir() {
        return 0;
    }
    let blockmap_hashes = parse_appx_blockmap_asset_hashes(&install.app_path);
    if blockmap_hashes.is_empty() {
        return 0;
    }
    let assets_dir = frontend_assets_dir(&install.resources_path);
    if !assets_dir.is_dir() {
        return 0;
    }
    let Ok(v_entries) = fs::read_dir(backup_root) else {
        return 0;
    };
    let mut version_dirs = v_entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    version_dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));

    let mut corrupted = HashSet::new();
    for version_dir in version_dirs {
        let Ok(entries) = fs::read_dir(&version_dir) else {
            continue;
        };
        let mut candidates = entries
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
            if manifest.version != 1 {
                continue;
            }
            for file in &manifest.files {
                if !file.existed {
                    continue;
                }
                let Some(file_name) = file.target.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if corrupted.contains(file_name) {
                    continue;
                }
                let Some(expected_sha) = blockmap_hashes.get(file_name) else {
                    continue;
                };
                if file.sha256.as_deref() != Some(expected_sha.as_str()) {
                    continue;
                }
                let current_target = assets_dir.join(file_name);
                if !current_target.is_file() {
                    continue;
                }
                if let Ok(current_sha) = sha256_file(&current_target) {
                    if &current_sha != expected_sha {
                        corrupted.insert(file_name.to_string());
                    }
                }
            }
        }
    }
    corrupted.len()
}

/// 当 Microsoft Store 对 MSIX 包做 64KB 增量块更新时，如果上一版本被中文化改写过，
/// 同 hash 文件名的 `.js` chunk 可能发生跨块拼接损坏（如 `SyntaxError: Unexpected token 'get'`）。
/// 依据当前版本 `AppxBlockMap.xml` 的 `FileHash`，从历史版本的干净备份修复损坏的 `.js` 文件。
fn heal_cross_version_corrupted_assets(
    backup_root: &Path,
    install: &LocalizationInstall,
    log: &JobLog,
) -> AppResult<usize> {
    if !backup_root.is_dir() {
        return Ok(0);
    }
    let blockmap_hashes = parse_appx_blockmap_asset_hashes(&install.app_path);
    if blockmap_hashes.is_empty() {
        return Ok(0);
    }
    let assets_dir = frontend_assets_dir(&install.resources_path);
    if !assets_dir.is_dir() {
        return Ok(0);
    }
    let mut corrupted_targets: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (file_name, expected_sha) in &blockmap_hashes {
        let current_target = assets_dir.join(file_name);
        if !current_target.is_file() {
            continue;
        }
        if let Ok(current_sha) = sha256_file(&current_target) {
            if &current_sha != expected_sha {
                corrupted_targets.insert(file_name.clone(), expected_sha.clone());
            }
        }
    }
    if corrupted_targets.is_empty() {
        return Ok(0);
    }

    let mut version_dirs = fs::read_dir(backup_root)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    version_dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));

    let mut healed = 0usize;
    for version_dir in version_dirs {
        if corrupted_targets.is_empty() {
            break;
        }
        let Ok(entries) = fs::read_dir(&version_dir) else {
            continue;
        };
        let mut candidates = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && path.join("manifest.json").is_file())
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
        for candidate in candidates {
            if corrupted_targets.is_empty() {
                break;
            }
            let Ok(bytes) = fs::read(candidate.join("manifest.json")) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_slice::<LocalizationBackupManifest>(&bytes) else {
                continue;
            };
            if manifest.version != 1 {
                continue;
            }
            for file in &manifest.files {
                if !file.existed {
                    continue;
                }
                let Some(file_name) = file.target.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(expected_sha) = corrupted_targets.get(file_name).cloned() else {
                    continue;
                };
                if file.sha256.as_deref() != Some(expected_sha.as_str()) {
                    continue;
                }
                let Some(backup_name) = file.backup_file.as_deref() else {
                    continue;
                };
                if backup_name.is_empty()
                    || backup_name.contains(['/', '\\'])
                    || backup_name == ".."
                {
                    continue;
                }
                let backup_source = candidate.join(backup_name);
                let Ok(backup_bytes) = fs::read(&backup_source) else {
                    continue;
                };
                if hex::encode(Sha256::digest(&backup_bytes)) != expected_sha {
                    continue;
                }
                let current_target = assets_dir.join(file_name);
                atomic_write(&current_target, &backup_bytes)?;
                corrupted_targets.remove(file_name);
                healed += 1;
            }
        }
    }
    if healed > 0 {
        log.line(&format!(
            "已根据 AppxBlockMap 校验并从历史备份修复 {} 个跨版本增量更新损坏的前端文件",
            healed
        ));
    }
    Ok(healed)
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
    let _ = heal_cross_version_corrupted_assets(backup_root, install, log)?;
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
