// 前端 bundle 补丁引擎（纯函数：文本进、文本 + 统计出），由 desktop_localization.rs `include!`。
//
// 同一套扫描逻辑用于：安装前基线检测、写入前自检、写入后校验、状态判定。

/// 语言白名单注册结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LanguageRegistration {
    /// 找到的白名单数量。
    lists_found: usize,
    /// 本次实际被改写的白名单数量（已含 zh-CN 的不计，保证幂等）。
    lists_changed: usize,
}

/// 对文本里**每一份**语言白名单补上 zh-CN（新版 bundle 同一文件里有 `w_` 与 `M_/N_` 两份）。
fn register_language(text: &str) -> (String, LanguageRegistration) {
    let mut registration = LanguageRegistration::default();
    let mut result = String::with_capacity(text.len() + 16);
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(BASE_LANGUAGES) {
        let start = cursor + relative;
        let after = start + BASE_LANGUAGES.len();
        let Some(end_offset) = text[after..].find(']') else {
            break;
        };
        let end = after + end_offset;
        let existing = &text[start..=end];
        let without_chinese = existing
            .replace(",\"zh-CN\"", "")
            .replace(",\"zh-TW\"", "")
            .replace(",\"zh-HK\"", "");
        let replacement = format!("{},\"zh-CN\"]", without_chinese.trim_end_matches(']'));
        registration.lists_found += 1;
        if replacement != existing {
            registration.lists_changed += 1;
        }
        result.push_str(&text[cursor..start]);
        result.push_str(&replacement);
        cursor = end + 1;
    }
    result.push_str(&text[cursor..]);
    (result, registration)
}

/// 移除文本里每一份语言白名单中的 `zh-CN` / `zh-TW` / `zh-HK`（用于无同版本备份时卸载中文化）。
fn unregister_language(text: &str) -> (String, usize) {
    let mut lists_changed = 0usize;
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(BASE_LANGUAGES) {
        let start = cursor + relative;
        let after = start + BASE_LANGUAGES.len();
        let Some(end_offset) = text[after..].find(']') else {
            break;
        };
        let end = after + end_offset;
        let existing = &text[start..=end];
        let replacement = existing
            .replace(",\"zh-CN\"", "")
            .replace(",\"zh-TW\"", "")
            .replace(",\"zh-HK\"", "");
        if replacement != existing {
            lists_changed += 1;
        }
        result.push_str(&text[cursor..start]);
        result.push_str(&replacement);
        cursor = end + 1;
    }
    result.push_str(&text[cursor..]);
    (result, lists_changed)
}

/// 返回 `(白名单总数, 已含 zh-CN 的数量)`。
fn count_language_lists(text: &str) -> (usize, usize) {
    let mut total = 0;
    let mut with_zh = 0;
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(BASE_LANGUAGES) {
        let start = cursor + relative;
        let after = start + BASE_LANGUAGES.len();
        let Some(end_offset) = text[after..].find(']') else {
            break;
        };
        let end = after + end_offset;
        total += 1;
        if text[start..=end].contains(",\"zh-CN\"") {
            with_zh += 1;
        }
        cursor = end + 1;
    }
    (total, with_zh)
}

/// 纯小写单词（`tokens`/`matches`/`seconds`…）在 bundle 里多是逻辑标识，替换会改坏程序。
fn is_lowercase_word_source(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn load_hardcoded_replacements(path: &Path) -> AppResult<Vec<(String, String)>> {
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let array = value
        .as_array()
        .ok_or_else(|| AppError::Config("硬编码翻译映射必须是数组".to_string()))?;
    if array.len() > MAX_HARDCODED_REPLACEMENTS {
        return Err(AppError::Config("硬编码翻译映射条目过多".to_string()));
    }
    let mut replacements = Vec::new();
    for item in array {
        let pair = item
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(|| AppError::Config("硬编码翻译映射条目格式无效".to_string()))?;
        let source = pair[0]
            .as_str()
            .ok_or_else(|| AppError::Config("硬编码翻译源文本无效".to_string()))?;
        let target = pair[1]
            .as_str()
            .ok_or_else(|| AppError::Config("硬编码翻译目标文本无效".to_string()))?;
        if source.is_empty()
            || target.is_empty()
            || source == target
            || source.len() > 1_000
            || target.len() > 2_000
            || source.contains('\n')
            || is_lowercase_word_source(source)
            || source == "\"Search\""
        {
            continue;
        }
        replacements.push((source.to_string(), target.to_string()));
    }
    replacements.sort_by_key(|item| std::cmp::Reverse(item.0.len()));
    Ok(replacements)
}

/// 上下文保护模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextMode {
    /// 安装时：图标键 + `as/component/displayName/glyph/name/role/type`。
    Upstream,
    /// 自检时：只看图标键（`name:"简体中文"` 在原版里是合法的）。
    IconOnly,
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'-')
}

fn is_protected_key(identifier: &str, mode: ContextMode) -> bool {
    let icon_key = ["Icon", "icon", "IconName", "iconName"]
        .iter()
        .any(|suffix| identifier.ends_with(suffix));
    if icon_key {
        return true;
    }
    mode == ContextMode::Upstream
        && matches!(
            identifier,
            "as" | "component" | "displayName" | "glyph" | "name" | "role" | "type"
        )
}

/// 键与字面量之间的片段是否仍属于「同一个值表达式」：
/// 不含 `, ; { }`，且每个 `:` 都是三元分支 `cond?"A":` 的冒号。
fn is_same_value_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let mut index = 0;
    // 'q' 上一个有效 token 是 `?`；'s' 是紧跟在 `?` 之后的引号串；'S' 是其它引号串；'o' 其它。
    let mut previous = 'o';
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'"' => {
                let Some(close) = bytes[index + 1..].iter().position(|b| *b == b'"') else {
                    return false;
                };
                if close > 60 {
                    return false;
                }
                previous = if previous == 'q' { 's' } else { 'S' };
                index += close + 2;
            }
            b',' | b';' | b'{' | b'}' => return false,
            b':' => {
                if previous != 's' {
                    return false;
                }
                previous = 'o';
                index += 1;
            }
            b'?' => {
                previous = 'q';
                index += 1;
            }
            _ => {
                previous = 'o';
                index += 1;
            }
        }
    }
    true
}

/// 位于 `literal_start` 的字符串字面量是否处在「结构性」上下文（图标名等）。
fn is_structural_js_literal_context(text: &str, literal_start: usize, mode: ContextMode) -> bool {
    let mut low = literal_start.saturating_sub(96);
    while low < literal_start && !text.is_char_boundary(low) {
        low += 1;
    }
    let prefix = &text[low..literal_start];
    let bytes = prefix.as_bytes();
    for (position, byte) in bytes.iter().enumerate() {
        if !matches!(byte, b':' | b'=') {
            continue;
        }
        if *byte == b'=' {
            // 跳过 `==` `===` `=>` `<=` `>=` `!=`，只认赋值/解构默认值。
            let next = bytes.get(position + 1).copied();
            let prev = position.checked_sub(1).map(|index| bytes[index]);
            if matches!(next, Some(b'=') | Some(b'>'))
                || matches!(prev, Some(b'=') | Some(b'!') | Some(b'<') | Some(b'>'))
            {
                continue;
            }
        }
        let mut end = position;
        while end > 0 && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && is_identifier_byte(bytes[start - 1]) {
            start -= 1;
        }
        if start == end {
            continue;
        }
        let Some(identifier) = prefix.get(start..end) else {
            continue;
        };
        if !is_protected_key(identifier, mode) {
            continue;
        }
        if let Some(segment) = prefix.get(position + 1..) {
            if is_same_value_segment(segment) {
                return true;
            }
        }
    }
    false
}

/// 逐处替换 `"源串"` → `"译文"`，处在结构性上下文的命中原样保留。返回实际替换次数。
fn apply_hardcoded_replacements(text: &mut String, replacements: &[(String, String)]) -> usize {
    let mut total = 0;
    for (source, target) in replacements {
        if source
            .chars()
            .any(|character| matches!(character, '\\' | '"' | '\'' | '`' | '=' | ';'))
            || source.contains("=>")
        {
            continue;
        }
        let Ok(source_json) = serde_json::to_string(source) else {
            continue;
        };
        if !text.contains(&source_json) {
            continue;
        }
        let Ok(target_json) = serde_json::to_string(target) else {
            continue;
        };
        let mut output = String::with_capacity(text.len());
        let mut last = 0;
        let mut replaced = 0;
        for (index, _) in text.match_indices(&source_json) {
            if is_structural_js_literal_context(text, index, ContextMode::Upstream) {
                continue;
            }
            output.push_str(&text[last..index]);
            output.push_str(&target_json);
            last = index + source_json.len();
            replaced += 1;
        }
        if replaced > 0 {
            output.push_str(&text[last..]);
            *text = output;
            total += replaced;
        }
    }
    total
}

/// 图标键后紧跟 CJK 字面量的数量（`icon:"添加"` 这类被改坏的图标名）。
fn count_icon_cjk(text: &str) -> usize {
    let bytes = text.as_bytes();
    text.match_indices('"')
        .filter(|(index, _)| {
            bytes
                .get(index + 1)
                .is_some_and(|byte| (0xE3..=0xE9).contains(byte))
                && is_structural_js_literal_context(text, *index, ContextMode::IconOnly)
        })
        .count()
}

/// 逻辑词被翻译的标记：`"令牌"in e`、`"匹配"in e`。
fn count_token_markers(text: &str) -> usize {
    text.matches("\"令牌\"in").count() + text.matches("\"匹配\"in").count()
}

/// 前端 bundle 的补丁状态统计。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PatchScan {
    js_files: usize,
    lists_total: usize,
    lists_with_zh: usize,
    icon_cjk: usize,
    token_marker: usize,
    skipped_non_utf8: usize,
}

impl PatchScan {
    fn add_text(&mut self, text: &str) {
        let (total, with_zh) = count_language_lists(text);
        self.lists_total += total;
        self.lists_with_zh += with_zh;
        self.icon_cjk += count_icon_cjk(text);
        self.token_marker += count_token_markers(text);
    }

    fn is_corrupted(&self) -> bool {
        self.icon_cjk > 0 || self.token_marker > 0
    }

    fn all_lists_registered(&self) -> bool {
        self.lists_total >= 1 && self.lists_with_zh == self.lists_total
    }
}

fn frontend_assets_dir(resources: &Path) -> PathBuf {
    resources.join("ion-dist").join("assets").join("v1")
}

/// 全量扫描 `assets/v1/*.js`。非 UTF-8 文件只计数，不报错。
fn scan_dir(assets: &Path) -> AppResult<PatchScan> {
    let entries = fs::read_dir(assets)
        .map_err(|_| AppError::Config(format!("未找到 Claude 前端资源目录: {}", assets.display())))?;
    let mut scan = PatchScan::default();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("js") {
            continue;
        }
        scan.js_files += 1;
        let bytes = fs::read(&path)
            .map_err(|error| AppError::Io(format!("读取前端资源失败 {}: {error}", path.display())))?;
        match std::str::from_utf8(&bytes) {
            Ok(text) => scan.add_text(text),
            Err(_) => scan.skipped_non_utf8 += 1,
        }
    }
    Ok(scan)
}

/// 写入前自检：补丁后图标 CJK / 逻辑词标记不得比原文更多。
fn verify_patched_sources(original: &str, patched: &str) -> Result<(), String> {
    let icon_before = count_icon_cjk(original);
    let icon_after = count_icon_cjk(patched);
    if icon_after > icon_before {
        return Err(format!(
            "图标名被翻译成中文（{icon_before} → {icon_after} 处），会导致按钮图标空白"
        ));
    }
    let token_before = count_token_markers(original);
    let token_after = count_token_markers(patched);
    if token_after > token_before {
        return Err(format!(
            "逻辑词被翻译（{token_before} → {token_after} 处），会导致界面崩溃"
        ));
    }
    Ok(())
}

fn dynamic_en_us_path(resources: &Path) -> PathBuf {
    resources
        .join("ion-dist")
        .join("i18n")
        .join("dynamic")
        .join("en-US.json")
}

fn dynamic_zh_cn_path(resources: &Path) -> PathBuf {
    resources
        .join("ion-dist")
        .join("i18n")
        .join("dynamic")
        .join("zh-CN.json")
}

/// 写入前计划：要写的文件 + 统计。
#[derive(Debug)]
struct InstallPlan {
    changes: Vec<FileChange>,
    stats: InstallStats,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct InstallStats {
    patched_js_count: usize,
    lists_total: usize,
    lists_registered: usize,
    dynamic_catalog: bool,
}

fn collect_install_changes(
    install: &LocalizationInstall,
    pack: &ValidatedPack,
    locale_paths: &[PathBuf],
    log: &JobLog,
) -> AppResult<InstallPlan> {
    let mut changes = Vec::new();
    let mut stats = InstallStats::default();
    for (source, target) in [
        (
            pack.resources.join(FRONTEND_FILE),
            install
                .resources_path
                .join("ion-dist")
                .join("i18n")
                .join("zh-CN.json"),
        ),
        (
            pack.resources.join(DESKTOP_FILE),
            install.resources_path.join("zh-CN.json"),
        ),
        (
            pack.resources.join(STATSIG_FILE),
            install
                .resources_path
                .join("ion-dist")
                .join("i18n")
                .join("statsig")
                .join("zh-CN.json"),
        ),
    ] {
        changes.push(FileChange {
            target,
            bytes: fs::read(source)?,
        });
    }

    // Claude 新版强制加载 dynamic catalog：缺 zh-CN.json 会让整个语言包回退英文。
    let dynamic_source = dynamic_en_us_path(&install.resources_path);
    if dynamic_source.is_file() {
        changes.push(FileChange {
            target: dynamic_zh_cn_path(&install.resources_path),
            bytes: fs::read(&dynamic_source)?,
        });
        stats.dynamic_catalog = true;
    } else {
        log.line("未找到 dynamic/en-US.json，跳过 dynamic catalog");
    }

    let replacements = load_hardcoded_replacements(&pack.resources.join(HARDCODED_FILE))?;
    let assets = frontend_assets_dir(&install.resources_path);
    let js_files = fs::read_dir(&assets)
        .map_err(|_| AppError::Config(format!("未找到 Claude 前端资源目录: {}", assets.display())))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("js"))
        .collect::<Vec<_>>();
    if js_files.is_empty() {
        return Err(AppError::Config("未找到 Claude 前端 JS bundle".to_string()));
    }
    for path in js_files {
        let original = fs::read_to_string(&path)
            .map_err(|_| AppError::Config(format!("无法读取 UTF-8 前端资源: {}", path.display())))?;
        let (mut patched, _registration) = register_language(&original);
        apply_hardcoded_replacements(&mut patched, &replacements);
        let (total, with_zh) = count_language_lists(&patched);
        if with_zh < total {
            return Err(AppError::Config(format!(
                "语言白名单未能全部注册 zh-CN（{with_zh}/{total}）: {}",
                path.display()
            )));
        }
        stats.lists_total += total;
        stats.lists_registered += with_zh;
        if patched != original {
            verify_patched_sources(&original, &patched).map_err(|reason| {
                AppError::Config(format!(
                    "前端补丁自检未通过 {}: {reason}，已中止，未写入任何文件",
                    path.display()
                ))
            })?;
            stats.patched_js_count += 1;
            changes.push(FileChange {
                target: path,
                bytes: patched.into_bytes(),
            });
        }
    }
    if stats.lists_total == 0 {
        return Err(AppError::Config(
            "未能注册 zh-CN，当前 Claude 前端结构可能已变化".to_string(),
        ));
    }
    log.line(&format!(
        "补丁计划：{} 个前端文件需要改写，{} 处语言白名单（{} 处已含 zh-CN）",
        stats.patched_js_count, stats.lists_total, stats.lists_registered
    ));

    for path in locale_paths {
        let mut value = if path.is_file() {
            read_json_file::<Value>(path)?.ok_or_else(|| {
                AppError::Config(format!("无法读取 Claude locale 配置: {}", path.display()))
            })?
        } else {
            serde_json::json!({})
        };
        let object = value.as_object_mut().ok_or_else(|| {
            AppError::Config(format!("Claude locale 配置不是 JSON 对象: {}", path.display()))
        })?;
        object.insert(
            "locale".to_string(),
            Value::String(LOCALIZATION_LOCALE.to_string()),
        );
        changes.push(FileChange {
            target: path.clone(),
            bytes: serde_json::to_vec_pretty(&value)?,
        });
    }

    let mut seen = HashSet::new();
    changes.retain(|change| seen.insert(change.target.clone()));
    Ok(InstallPlan { changes, stats })
}
