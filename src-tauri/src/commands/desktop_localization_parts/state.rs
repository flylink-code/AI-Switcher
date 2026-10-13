// 中文化状态判定（installed / partial / notInstalled）与全量扫描缓存。
// 由 desktop_localization.rs `include!`。

struct StateInputs {
    frontend: bool,
    desktop: bool,
    statsig: bool,
    dynamic_required: bool,
    dynamic_present: bool,
    locale_is_zh: bool,
    /// 仅在其它条件都就绪后才会去扫。
    scan: Option<PatchScan>,
    scan_error: Option<String>,
    last_install: Option<LastInstallInfo>,
    claude_version: String,
}

/// 判定状态并给出 `partial` 的逐条原因。
/// `installed` 当且仅当：三份资源文件齐 + dynamic（需要时）+ locale 为 zh-CN
/// + 每份语言白名单都含 zh-CN + 没有腐败标记。
fn evaluate_install_state(inputs: &StateInputs) -> (&'static str, Vec<String>) {
    let missing_files = [
        (inputs.frontend, "ion-dist/i18n/zh-CN.json"),
        (inputs.desktop, "zh-CN.json"),
        (inputs.statsig, "ion-dist/i18n/statsig/zh-CN.json"),
    ]
    .into_iter()
    .filter(|(present, _)| !present)
    .map(|(_, name)| name)
    .collect::<Vec<_>>();
    let nothing_installed = missing_files.len() == 3
        && !inputs.dynamic_present
        && !inputs.locale_is_zh;
    if nothing_installed {
        return ("notInstalled", Vec::new());
    }

    let mut reasons = Vec::new();
    if !missing_files.is_empty() {
        reasons.push(format!("缺少资源文件：{}", missing_files.join("、")));
    }
    if inputs.dynamic_required && !inputs.dynamic_present {
        reasons.push(
            "缺少 ion-dist/i18n/dynamic/zh-CN.json（Claude 新版强制加载，缺失会回退英文）"
                .to_string(),
        );
    }
    if !inputs.locale_is_zh {
        reasons.push("Claude 配置 locale 不是 zh-CN".to_string());
    }
    if let Some(error) = &inputs.scan_error {
        reasons.push(format!("无法校验前端补丁：{error}"));
    }
    if let Some(scan) = &inputs.scan {
        if !scan.all_lists_registered() {
            reasons.push(format!(
                "语言白名单 {}/{} 处已注册 zh-CN",
                scan.lists_with_zh, scan.lists_total
            ));
        }
        if scan.is_corrupted() {
            reasons.push(format!(
                "检测到旧版中文化遗留的损坏内容（图标名被翻译 {} 处 / 令牌等逻辑词被替换 {} 处），请重新安装修复",
                scan.icon_cjk, scan.token_marker
            ));
        }
    }
    if let Some(last) = &inputs.last_install {
        if last.claude_version != inputs.claude_version {
            reasons.push(format!(
                "Claude 已从 {} 更新到 {}，需要重新安装中文化",
                last.claude_version, inputs.claude_version
            ));
        }
    }
    if reasons.is_empty() {
        ("installed", reasons)
    } else {
        ("partial", reasons)
    }
}

/// 扫描缓存键：资源目录 + 前端/ dynamic 语言文件 + assets 目录的 (长度, 修改时间)。
///
/// 前提：assets 目录的 mtime 只在文件被创建/重命名/删除时变化，原地改写 JS 不会使其失效。
/// 安装/恢复一律经 `atomic_write`（临时文件 + rename）写入，且同时会改动 zh-CN.json，
/// 所以缓存会随之失效；如果将来有绕过 `atomic_write` 的写入路径，需要同步修改这里。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ScanKey {
    resources_path: PathBuf,
    stamps: Vec<Option<(u64, Option<SystemTime>)>>,
}

fn file_stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}

fn scan_key(resources: &Path) -> ScanKey {
    ScanKey {
        resources_path: resources.to_path_buf(),
        stamps: vec![
            file_stamp(
                &resources
                    .join("ion-dist")
                    .join("i18n")
                    .join("zh-CN.json"),
            ),
            file_stamp(&dynamic_zh_cn_path(resources)),
            file_stamp(&frontend_assets_dir(resources)),
        ],
    }
}

#[derive(Default)]
struct ScanCache {
    entry: Option<(ScanKey, PatchScan)>,
}

impl ScanCache {
    fn get_or_scan(
        &mut self,
        key: ScanKey,
        scan: impl FnOnce() -> AppResult<PatchScan>,
    ) -> AppResult<PatchScan> {
        if let Some((cached_key, cached)) = &self.entry {
            if *cached_key == key {
                return Ok(cached.clone());
            }
        }
        let fresh = scan()?;
        self.entry = Some((key, fresh.clone()));
        Ok(fresh)
    }
}

static SCAN_CACHE: Mutex<Option<ScanCache>> = Mutex::new(None);

fn cached_scan(resources: &Path) -> AppResult<PatchScan> {
    let key = scan_key(resources);
    let mut guard = SCAN_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .get_or_insert_with(ScanCache::default)
        .get_or_scan(key, || scan_dir(&frontend_assets_dir(resources)))
}
