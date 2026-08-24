//! Deep Link / clipboard import for providers and MCP servers.
//!
//! Protocol:
//! `ai-switcher://v1/import?resource=provider|mcp&payload=<urlsafe-base64-json>`
//!
//! Payloads are credential-free export subsets. Plaintext API keys in payload
//! are stripped and surfaced as warnings.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use url::Url;

use crate::error::{AppError, AppResult};
use crate::mcp::McpServerInput;
use crate::provider::{
    normalize_base_url, ProviderExportBundle, ProviderExportEntry, ProviderInput, ProviderKind,
};

pub const DEEPLINK_SCHEME: &str = "ai-switcher";
pub const DEEPLINK_IMPORT_EVENT: &str = "deeplink-import";
pub const DEEPLINK_ERROR_EVENT: &str = "deeplink-error";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportResource {
    Provider,
    Mcp,
    Skill,
}

impl ImportResource {
    fn parse(value: &str) -> AppResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "provider" | "providers" => Ok(Self::Provider),
            "mcp" | "mcp_server" | "mcp_servers" => Ok(Self::Mcp),
            "skill" | "skills" => Ok(Self::Skill),
            other => Err(AppError::Config(format!("不支持的 Deep Link 资源类型: {other}"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Mcp => "mcp",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewItem {
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub resource: ImportResource,
    pub source: String,
    pub items: Vec<ImportPreviewItem>,
    pub warnings: Vec<String>,
    /// Sanitized JSON ready for confirm import.
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpExportBundle {
    pub version: u32,
    pub servers: Vec<McpExportEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpExportEntry {
    pub name: String,
    #[serde(default)]
    pub server_config: Value,
    #[serde(default)]
    pub enabled_claude_code: bool,
    #[serde(default)]
    pub enabled_claude_desktop: bool,
    #[serde(default)]
    pub enabled_codex: bool,
    #[serde(default)]
    pub enabled_opencode: bool,
    #[serde(default)]
    pub enabled_pi: bool,
    #[serde(default)]
    pub enabled_cline: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeeplinkImportResult {
    pub resource: ImportResource,
    pub imported: usize,
    pub skipped: usize,
}

/// Parse a deeplink URL or raw JSON clipboard/file text into a preview DTO.
pub fn parse_import_text(raw: &str) -> AppResult<ImportPreview> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("导入内容为空".to_string()));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("ai-switcher://") || lower.starts_with("ccswitch://") || lower.starts_with("aiswitcher://") {
        return parse_deeplink_url(trimmed);
    }
    parse_import_json(trimmed, "clipboard")
}

pub fn parse_deeplink_url(raw: &str) -> AppResult<ImportPreview> {
    let url = Url::parse(raw.trim())
        .map_err(|error| AppError::Config(format!("Deep Link URL 无效: {error}")))?;
    let scheme = url.scheme().to_ascii_lowercase();
    if scheme != "ai-switcher" && scheme != "ccswitch" && scheme != "aiswitcher" {
        return Err(AppError::Config(format!(
            "Deep Link scheme 必须是 ai-switcher:// 或 ccswitch://"
        )));
    }
    let host = url.host_str().unwrap_or("");
    let path = url.path().trim_matches('/');
    let versioned = if host.eq_ignore_ascii_case("v1") {
        path
    } else if path.eq_ignore_ascii_case("v1/import") || path.eq_ignore_ascii_case("import") {
        "import"
    } else {
        return Err(AppError::Config(
            "Deep Link 路径应为 ai-switcher://v1/import".to_string(),
        ));
    };
    if !versioned.eq_ignore_ascii_case("import") && !versioned.is_empty() {
        return Err(AppError::Config(
            "Deep Link 路径应为 ai-switcher://v1/import".to_string(),
        ));
    }

    let mut resource = None;
    let mut payload = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "resource" => resource = Some(ImportResource::parse(&value)?),
            "payload" => payload = Some(value.into_owned()),
            _ => {}
        }
    }
    let resource = resource.ok_or_else(|| AppError::Config("缺少 resource 参数".to_string()))?;
    let payload =
        payload.ok_or_else(|| AppError::Config("缺少 payload 参数".to_string()))?;
    let json = decode_payload(&payload)?;
    build_preview(resource, json, "deeplink")
}

pub fn parse_import_json(raw: &str, source: &str) -> AppResult<ImportPreview> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| AppError::Config("导入 JSON 无效".to_string()))?;
    let resource = detect_resource(&value)?;
    build_preview(resource, value, source)
}

fn detect_resource(value: &Value) -> AppResult<ImportResource> {
    if value.get("providers").is_some() {
        return Ok(ImportResource::Provider);
    }
    if value.get("servers").is_some() || value.get("mcpServers").is_some() {
        return Ok(ImportResource::Mcp);
    }
    if value.get("skills").is_some() {
        return Ok(ImportResource::Skill);
    }
    Err(AppError::Config(
        "无法识别导入类型（需要 providers, servers 或 skills）".to_string(),
    ))
}

fn decode_payload(payload: &str) -> AppResult<Value> {
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .or_else(|_| STANDARD.decode(payload.as_bytes()))
        .map_err(|_| AppError::Config("Deep Link payload 不是有效 Base64".to_string()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| AppError::Config("Deep Link payload 不是有效 UTF-8".to_string()))?;
    serde_json::from_str(&text)
        .map_err(|_| AppError::Config("Deep Link payload 不是有效 JSON".to_string()))
}

fn encode_payload(value: &Value) -> AppResult<String> {
    let json = serde_json::to_vec(value)?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

fn build_preview(resource: ImportResource, value: Value, source: &str) -> AppResult<ImportPreview> {
    match resource {
        ImportResource::Provider => preview_providers(value, source),
        ImportResource::Mcp => preview_mcp(value, source),
        ImportResource::Skill => preview_skills(value, source),
    }
}

fn preview_providers(value: Value, source: &str) -> AppResult<ImportPreview> {
    let mut warnings = Vec::new();
    let mut bundle: ProviderExportBundle = serde_json::from_value(value)
        .map_err(|_| AppError::Config("供应商导入结构无效".to_string()))?;
    if bundle.version == 0 {
        bundle.version = 1;
    }
    if bundle.version != 1 {
        return Err(AppError::Config(format!(
            "不支持的供应商导入版本: {}",
            bundle.version
        )));
    }
    if bundle.providers.is_empty() {
        return Err(AppError::Config("供应商列表为空".to_string()));
    }

    let mut items = Vec::new();
    let mut sanitized = Vec::new();
    for mut entry in bundle.providers {
        if provider_entry_has_secret(&entry) {
            warnings.push(format!(
                "「{}」包含密钥字段，已清空后导入",
                entry.name
            ));
            strip_provider_secrets(&mut entry);
        }
        let _ = normalize_base_url(&entry.base_url)?;
        items.push(ImportPreviewItem {
            name: entry.name.clone(),
            summary: format!(
                "{} · {} · group {}",
                entry.target_app.as_str(),
                entry.protocol_type.as_str(),
                entry.failover_group
            ),
            detail: serde_json::to_value(&entry)?,
        });
        sanitized.push(entry);
    }

    Ok(ImportPreview {
        resource: ImportResource::Provider,
        source: source.to_string(),
        items,
        warnings,
        payload: serde_json::to_value(ProviderExportBundle {
            version: 1,
            providers: sanitized,
        })?,
    })
}

fn provider_entry_has_secret(entry: &ProviderExportEntry) -> bool {
    let Ok(value) = serde_json::to_value(entry) else {
        return false;
    };
    value
        .as_object()
        .map(|object| {
            object.keys().any(|key| {
                let lower = key.to_ascii_lowercase();
                lower.contains("api_key")
                    || lower.contains("apikey")
                    || lower.contains("secret")
                    || lower.contains("token")
                    || lower.contains("password")
            })
        })
        .unwrap_or(false)
}

fn strip_provider_secrets(entry: &mut ProviderExportEntry) {
    // Export entry type intentionally has no key fields; keep for forward-compat maps.
    let _ = entry;
}

fn preview_mcp(value: Value, source: &str) -> AppResult<ImportPreview> {
    let mut warnings = Vec::new();
    let mut bundle = normalize_mcp_bundle(value)?;
    if bundle.servers.is_empty() {
        return Err(AppError::Config("MCP 列表为空".to_string()));
    }

    let mut items = Vec::new();
    for entry in &mut bundle.servers {
        if strip_mcp_secrets(&mut entry.server_config) {
            warnings.push(format!(
                "「{}」的 env/headers 含敏感字段，已清空",
                entry.name
            ));
        }
        items.push(ImportPreviewItem {
            name: entry.name.clone(),
            summary: mcp_summary(&entry.server_config),
            detail: serde_json::to_value(entry)?,
        });
    }

    Ok(ImportPreview {
        resource: ImportResource::Mcp,
        source: source.to_string(),
        items,
        warnings,
        payload: serde_json::to_value(bundle)?,
    })
}

fn normalize_mcp_bundle(value: Value) -> AppResult<McpExportBundle> {
    if let Ok(bundle) = serde_json::from_value::<McpExportBundle>(value.clone()) {
        let version = if bundle.version == 0 { 1 } else { bundle.version };
        if version != 1 {
            return Err(AppError::Config(format!(
                "不支持的 MCP 导入版本: {version}"
            )));
        }
        return Ok(McpExportBundle {
            version: 1,
            servers: bundle.servers,
        });
    }
    if let Some(map) = value.get("mcpServers").and_then(Value::as_object) {
        let servers = map
            .iter()
            .map(|(name, config)| McpExportEntry {
                name: name.clone(),
                server_config: config.clone(),
                enabled_claude_code: false,
                enabled_claude_desktop: false,
                enabled_codex: false,
                enabled_opencode: false,
                enabled_pi: false,
                enabled_cline: false,
            })
            .collect();
        return Ok(McpExportBundle {
            version: 1,
            servers,
        });
    }
    Err(AppError::Config("MCP 导入结构无效".to_string()))
}

fn strip_mcp_secrets(config: &mut Value) -> bool {
    let Some(object) = config.as_object_mut() else {
        return false;
    };
    let mut stripped = false;
    if let Some(env) = object.get_mut("env").and_then(Value::as_object_mut) {
        if !env.is_empty() {
            *env = Map::new();
            stripped = true;
        }
    }
    if let Some(headers) = object.get_mut("headers").and_then(Value::as_object_mut) {
        if !headers.is_empty() {
            *headers = Map::new();
            stripped = true;
        }
    }
    for key in ["apiKey", "api_key", "token", "authorization", "password"] {
        if object.remove(key).is_some() {
            stripped = true;
        }
    }
    stripped
}

fn mcp_summary(config: &Value) -> String {
    if let Some(command) = config.get("command").and_then(Value::as_str) {
        return format!("stdio · {command}");
    }
    if let Some(url) = config.get("url").and_then(Value::as_str) {
        return format!("remote · {url}");
    }
    "mcp".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExportBundle {
    pub version: u32,
    pub skills: Vec<SkillExportEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExportEntry {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
}

fn preview_skills(value: Value, source: &str) -> AppResult<ImportPreview> {
    let bundle: SkillExportBundle = serde_json::from_value(value)
        .map_err(|_| AppError::Config("Skill 导入结构无效".to_string()))?;
    if bundle.skills.is_empty() {
        return Err(AppError::Config("Skill 列表为空".to_string()));
    }

    let mut items = Vec::new();
    for entry in &bundle.skills {
        items.push(ImportPreviewItem {
            name: entry.name.clone(),
            summary: format!("GitHub Skill · {}", entry.url),
            detail: serde_json::to_value(entry)?,
        });
    }

    Ok(ImportPreview {
        resource: ImportResource::Skill,
        source: source.to_string(),
        items,
        warnings: Vec::new(),
        payload: serde_json::to_value(bundle)?,
    })
}

pub fn build_skill_share_link(name: &str, url: &str) -> AppResult<String> {
    let payload = encode_payload(&serde_json::to_value(SkillExportBundle {
        version: 1,
        skills: vec![SkillExportEntry {
            name: name.to_string(),
            url: url.to_string(),
            description: String::new(),
        }],
    })?)?;
    Ok(format!(
        "{DEEPLINK_SCHEME}://v1/import?resource=skill&payload={payload}"
    ))
}

pub fn build_provider_share_link(entry: &ProviderExportEntry) -> AppResult<String> {
    let mut sanitized = entry.clone();
    strip_provider_secrets(&mut sanitized);
    let payload = encode_payload(&serde_json::to_value(ProviderExportBundle {
        version: 1,
        providers: vec![sanitized],
    })?)?;
    Ok(format!(
        "{DEEPLINK_SCHEME}://v1/import?resource=provider&payload={payload}"
    ))
}

pub fn build_mcp_share_link(entry: &McpExportEntry) -> AppResult<String> {
    let mut sanitized = entry.clone();
    let _ = strip_mcp_secrets(&mut sanitized.server_config);
    let payload = encode_payload(&serde_json::to_value(McpExportBundle {
        version: 1,
        servers: vec![sanitized],
    })?)?;
    Ok(format!(
        "{DEEPLINK_SCHEME}://v1/import?resource=mcp&payload={payload}"
    ))
}

pub fn provider_inputs_from_preview(preview: &ImportPreview) -> AppResult<Vec<ProviderInput>> {
    let bundle: ProviderExportBundle = serde_json::from_value(preview.payload.clone())
        .map_err(|_| AppError::Config("预览 payload 无效".to_string()))?;
    let mut inputs = Vec::new();
    for entry in bundle.providers {
        inputs.push(ProviderInput {
            id: None,
            name: entry.name,
            base_url: normalize_base_url(&entry.base_url)?,
            api_key: String::new(),
            clear_api_key: false,
            model: entry.model,
            model_context_window: entry.model_context_window,
            auto_review_model_override: None,
            web_search_enabled: entry.web_search_enabled,
            model_mapping: entry.model_mapping,
            protocol_type: entry.protocol_type,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: entry.target_app,
            notes: entry.notes,
            failover_group: entry.failover_group,
            failover_models: entry.failover_models,
            hidden_models: entry.hidden_models,
            thinking_config: entry.thinking_config,
            custom_headers: entry.custom_headers,
        });
    }
    Ok(inputs)
}

pub fn mcp_inputs_from_preview(preview: &ImportPreview) -> AppResult<Vec<McpServerInput>> {
    let bundle: McpExportBundle = serde_json::from_value(preview.payload.clone())
        .map_err(|_| AppError::Config("预览 payload 无效".to_string()))?;
    Ok(bundle
        .servers
        .into_iter()
        .map(|entry| McpServerInput {
            id: None,
            name: entry.name,
            server_config: entry.server_config,
            enabled_claude_code: entry.enabled_claude_code,
            enabled_claude_desktop: entry.enabled_claude_desktop,
            enabled_codex: entry.enabled_codex,
            enabled_opencode: entry.enabled_opencode,
            enabled_pi: entry.enabled_pi,
            enabled_cline: entry.enabled_cline,
        })
        .collect())
}

pub fn import_result_label(resource: ImportResource, imported: usize, skipped: usize) -> DeeplinkImportResult {
    DeeplinkImportResult {
        resource,
        imported,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeModelMapping, ProtocolType, ProviderTarget};

    #[test]
    fn roundtrips_provider_share_link() {
        let entry = ProviderExportEntry {
            name: "Demo".into(),
            base_url: "https://api.example.test/v1".into(),
            model: "gpt-5".into(),
            model_context_window: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::OpenAiChat,
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            failover_group: 1,
            failover_models: vec!["gpt-5".into()],
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
        };
        let link = build_provider_share_link(&entry).unwrap();
        assert!(link.starts_with("ai-switcher://v1/import?resource=provider&payload="));
        let preview = parse_deeplink_url(&link).unwrap();
        assert_eq!(preview.resource, ImportResource::Provider);
        assert_eq!(preview.items.len(), 1);
        assert_eq!(preview.items[0].name, "Demo");
    }

    #[test]
    fn clipboard_json_preview_works() {
        let json = json!({
            "version": 1,
            "providers": [{
                "name": "FromClip",
                "baseUrl": "https://clip.example.test/v1",
                "model": "m1",
                "protocolType": "openai_chat",
                "targetApp": "claude_code"
            }]
        });
        let preview = parse_import_text(&json.to_string()).unwrap();
        assert_eq!(preview.source, "clipboard");
        assert_eq!(preview.items[0].name, "FromClip");
    }

    #[test]
    fn mcp_env_is_stripped() {
        let json = json!({
            "version": 1,
            "servers": [{
                "name": "server",
                "serverConfig": {
                    "command": "npx",
                    "env": { "API_KEY": "secret" }
                }
            }]
        });
        let preview = parse_import_json(&json.to_string(), "clipboard").unwrap();
        assert!(!preview.warnings.is_empty());
        let env = preview.payload["servers"][0]["serverConfig"]["env"]
            .as_object()
            .unwrap();
        assert!(env.is_empty());
    }
}
