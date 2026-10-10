use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDriftField {
    pub field: String,
    pub current_present: bool,
    pub applied_present: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDriftReport {
    pub target: ProviderTarget,
    pub status: String,
    pub fields: Vec<ConfigDriftField>,
    pub revision: String,
}

fn drift_provider(state: &AppState, target: ProviderTarget) -> AppResult<Option<Provider>> {
    state.db.with_read_conn(|conn| {
        if let Some(binding) = crate::database::dao::gateway::binding_for_target(conn, target)? {
            if binding.mode == "direct" {
                return crate::database::dao::gateway::provider_from_upstream(
                    conn, &binding.direct_upstream_id, target,
                ).map(Some);
            }
        }
        dao::get_current_provider(conn, target)
    })
}

fn codex_drift_fields() -> AppResult<BTreeMap<String, Option<Value>>> {
    let path = crate::config::get_codex_config_path();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    parse_codex_drift_fields(&text)
}

fn parse_codex_drift_fields(text: &str) -> AppResult<BTreeMap<String, Option<Value>>> {
    let doc = text.parse::<toml_edit::DocumentMut>()
        .map_err(|_| AppError::Config("Codex 配置无法解析，不能检测配置漂移".into()))?;
    // 比较 TOML 语义值，注释、引号风格和空白不算配置漂移。
    let field_value = |item: &toml_edit::Item| {
        if let Some(value) = item.as_str() {
            Value::String(value.to_string())
        } else if let Some(value) = item.as_bool() {
            Value::Bool(value)
        } else {
            Value::String(item.to_string())
        }
    };
    let mut fields = BTreeMap::new();
    for key in ["model_provider", "model"] {
        fields.insert(key.to_string(), doc.get(key).map(field_value));
    }
    let entry = doc.get("model_providers")
        .and_then(|item| item.get(codex::MANAGED_PROVIDER_ID));
    for key in ["base_url", "wire_api", "requires_openai_auth"] {
        fields.insert(format!("model_providers.ai_switcher.{key}"),
            entry.and_then(|item| item.get(key)).map(field_value));
    }
    Ok(fields)
}

const CODEX_DRIFT_BASELINE: &str = "config_drift.codex_applied";

fn save_codex_drift_baseline(state: &AppState) -> AppResult<()> {
    let fields = codex_drift_fields()?;
    let value = serde_json::to_string(&fields)?;
    state.db.with_conn(|conn| set_setting(conn, CODEX_DRIFT_BASELINE, &value))
}

fn drift_changes(
    current: &BTreeMap<String, Option<Value>>,
    applied: &BTreeMap<String, Option<Value>>,
) -> Vec<ConfigDriftField> {
    applied.iter().filter_map(|(field, value)| {
        let now = current.get(field).cloned().flatten();
        (normalize_managed_value_for_key(field, now.clone())
            != normalize_managed_value_for_key(field, value.clone()))
            .then(|| ConfigDriftField {
                field: field.clone(), current_present: now.is_some(), applied_present: value.is_some(),
            })
    }).collect()
}

pub(crate) fn config_drift_report(state: &AppState, target: ProviderTarget) -> AppResult<ConfigDriftReport> {
    if !matches!(target, ProviderTarget::ClaudeCode | ProviderTarget::Codex) {
        return Err(AppError::Config("配置漂移检测目前仅支持 Claude Code 和 Codex".into()));
    }
    let provider = drift_provider(state, target)?;
    let current = if target == ProviderTarget::ClaudeCode { code_managed_fields()? } else { codex_drift_fields()? };
    let baseline = state.db.with_read_conn(|conn| get_setting(conn,
        if target == ProviderTarget::ClaudeCode { CODE_OWNERSHIP_KEY } else { CODEX_DRIFT_BASELINE }))?;
    let applied = match baseline.as_deref() {
        Some(raw) if target == ProviderTarget::ClaudeCode => Some(serde_json::from_str::<CodeOwnership>(raw)?.written),
        Some(raw) => Some(serde_json::from_str::<BTreeMap<String, Option<Value>>>(raw)?),
        None => None,
    };
    let fields = applied.as_ref().map(|applied| drift_changes(&current, applied)).unwrap_or_default();
    let status = if provider.is_none() { "unmanaged" } else if applied.is_none() { "unknown" }
        else if fields.is_empty() { "in_sync" } else { "drifted" };
    // 进程级盐防止凭据指纹可被离线字典枚举；IPC 永远不返回原始字段值。
    static SALT: OnceLock<String> = OnceLock::new();
    let mut digest = Sha256::new();
    digest.update(SALT.get_or_init(|| uuid::Uuid::new_v4().to_string()));
    digest.update(serde_json::to_vec(&(target, &current, &applied, &provider))?);
    // 同时覆盖配置中的非检测字段；预览期间这些字段被外部编辑也必须重新确认。
    let paths = if target == ProviderTarget::ClaudeCode {
        vec![crate::config::get_claude_settings_path()]
    } else {
        vec![crate::config::get_codex_config_path(), crate::config::get_codex_auth_path(),
            crate::config::get_codex_config_dir().join("ai-switcher-model-catalog.json")]
    };
    for path in paths {
        match std::fs::read(path) {
            Ok(bytes) => { digest.update([1]); digest.update((bytes.len() as u64).to_le_bytes()); digest.update(bytes); }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => digest.update([0]),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(ConfigDriftReport { target, status: status.into(), fields, revision: format!("{:x}", digest.finalize()) })
}

#[tauri::command]
pub fn get_agent_config_drift(target: ProviderTarget, state: tauri::State<'_, AppState>) -> AppResult<ConfigDriftReport> {
    config_drift_report(&state, target)
}

#[tauri::command]
pub async fn reapply_agent_config(
    target: ProviderTarget, revision: String, app: tauri::AppHandle, state: tauri::State<'_, AppState>,
) -> AppResult<ConfigDriftReport> {
    let _guard = agent_connection_lock().lock().await;
    let report = config_drift_report(&state, target)?;
    if report.revision != revision {
        return Err(AppError::Config("预览后配置已变化，请重新检查差异再应用".into()));
    }
    let provider = drift_provider(&state, target)?.ok_or_else(|| AppError::Config("官方原生连接没有可重新应用的托管配置".into()))?;
    apply_runtime_provider(&provider, Some(&app), &state).await?;
    config_drift_report(&state, target)
}

#[cfg(test)]
mod config_drift_tests {
    use super::*;

    #[test]
    fn codex_drift_ignores_toml_formatting() {
        let first = parse_codex_drift_fields("model = \"gpt-test\" # 旧注释\n[model_providers.ai_switcher]\nrequires_openai_auth = true\n").unwrap();
        let second = parse_codex_drift_fields("model='gpt-test'\n[model_providers.ai_switcher]\nrequires_openai_auth=true # 新注释\n").unwrap();
        assert!(drift_changes(&first, &second).is_empty());
        let changed = parse_codex_drift_fields("model='other'\n").unwrap();
        assert!(!drift_changes(&changed, &first).is_empty());
    }

    #[test]
    fn drift_preview_never_serializes_values() {
        let applied = BTreeMap::from([("ANTHROPIC_AUTH_TOKEN".into(), Some(Value::String("old-secret".into())))]);
        let current = BTreeMap::from([("ANTHROPIC_AUTH_TOKEN".into(), Some(Value::String("new-secret".into())))]);
        let fields = drift_changes(&current, &applied);
        assert_eq!(fields.len(), 1);
        let json = serde_json::to_string(&fields).unwrap();
        assert!(!json.contains("old-secret"));
        assert!(!json.contains("new-secret"));
    }

    #[test]
    fn drift_normalizes_endpoint_and_ignores_unowned_fields() {
        let applied = BTreeMap::from([("ANTHROPIC_BASE_URL".into(), Some(Value::String("https://example.test/".into())))]);
        let current = BTreeMap::from([
            ("ANTHROPIC_BASE_URL".into(), Some(Value::String("https://example.test".into()))),
            ("permissions.defaultMode".into(), Some(Value::String("ask".into()))),
        ]);
        assert!(drift_changes(&current, &applied).is_empty());
    }
}
