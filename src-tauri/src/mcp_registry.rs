//! Read-only integration with the official MCP Registry.
//!
//! Registry metadata is converted only when it maps cleanly to Claude's MCP
//! JSON format: npm stdio packages without declared setup variables, or remote
//! HTTP/SSE endpoints without URL templates. Other entries remain visible but
//! are marked for manual setup instead of guessing a configuration.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

const OFFICIAL_REGISTRY_URL: &str = "https://registry.modelcontextprotocol.io/v0.1/servers";
const REGISTRY_LIMIT: &str = "50";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryMcpServer {
    pub name: String,
    pub title: String,
    pub description: String,
    pub version: String,
    pub installable: bool,
    pub support_note: String,
}

#[derive(Debug, Deserialize)]
struct RegistryListResponse {
    #[serde(default)]
    servers: Vec<RegistryServerResponse>,
}

#[derive(Debug, Deserialize)]
struct RegistryServerResponse {
    server: RegistryServer,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RegistryServer {
    #[serde(default)]
    name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    packages: Vec<RegistryPackage>,
    #[serde(default)]
    remotes: Vec<RegistryRemote>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RegistryPackage {
    #[serde(default)]
    registry_type: String,
    #[serde(default)]
    identifier: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    transport: RegistryTransport,
    #[serde(default)]
    environment_variables: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RegistryTransport {
    #[serde(default, rename = "type")]
    kind: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RegistryRemote {
    #[serde(default, rename = "type", alias = "transportType")]
    kind: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    variables: Value,
}

pub async fn search_mcp_registry(query: &str) -> AppResult<Vec<RegistryMcpServer>> {
    let response = request_servers(query).await?;
    Ok(response.servers.into_iter().map(|entry| registry_card(entry.server)).collect())
}

pub async fn resolve_mcp_registry_server(name: &str) -> AppResult<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("MCP Registry 条目名称不能为空".to_string()));
    }
    let response = request_servers(name).await?;
    let server = response.servers.into_iter()
        .map(|entry| entry.server)
        .find(|server| server.name == name)
        .ok_or_else(|| AppError::Config(format!("官方 MCP Registry 未找到条目: {name}")))?;
    server_config(&server).map_err(AppError::Config)
}

async fn request_servers(query: &str) -> AppResult<RegistryListResponse> {
    let mut url = reqwest::Url::parse(OFFICIAL_REGISTRY_URL)
        .map_err(|e| AppError::Other(format!("MCP Registry 地址无效: {e}")))?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("limit", REGISTRY_LIMIT);
        if !query.trim().is_empty() {
            pairs.append_pair("search", query.trim());
        }
    }
    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Claude-Switcher")
        .send()
        .await
        .map_err(|e| AppError::Other(format!("访问官方 MCP Registry 失败: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::Config(format!("官方 MCP Registry 请求失败（HTTP {}）", response.status())));
    }
    let body = response.bytes().await
        .map_err(|e| AppError::Other(format!("读取官方 MCP Registry 响应失败: {e}")))?;
    serde_json::from_slice::<RegistryListResponse>(&body)
        .map_err(|e| AppError::Other(format!("解析官方 MCP Registry 响应失败: {e}")))
}

fn registry_card(server: RegistryServer) -> RegistryMcpServer {
    let support = server_config(&server);
    RegistryMcpServer {
        title: if server.title.trim().is_empty() { server.name.clone() } else { server.title.clone() },
        name: server.name,
        description: server.description,
        version: server.version,
        installable: support.is_ok(),
        support_note: support.err().unwrap_or_default(),
    }
}

fn server_config(server: &RegistryServer) -> Result<Value, String> {
    if let Some(remote) = server.remotes.iter().find(|remote| {
        matches!(remote.kind.as_str(), "streamable-http" | "http" | "sse")
            && !remote.url.trim().is_empty()
            && !remote.url.contains('{')
            && remote.variables.is_null()
    }) {
        let kind = if remote.kind == "streamable-http" { "http" } else { remote.kind.as_str() };
        return Ok(json!({ "type": kind, "url": remote.url }));
    }

    if let Some(package) = server.packages.iter().find(|package| {
        package.registry_type == "npm"
            && package.transport.kind == "stdio"
            && !package.identifier.trim().is_empty()
            && package.environment_variables.is_empty()
    }) {
        let package_name = if package.version.trim().is_empty() {
            package.identifier.clone()
        } else {
            format!("{}@{}", package.identifier, package.version)
        };
        #[cfg(windows)]
        return Ok(json!({ "command": "cmd", "args": ["/c", "npx", "-y", package_name] }));
        #[cfg(not(windows))]
        return Ok(json!({ "command": "npx", "args": ["-y", package_name] }));
    }

    Err("该条目需要密钥、URL 模板、非 npm 包管理器或手动配置，暂不支持一键加入".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_streamable_http_to_claude_http() {
        let server = RegistryServer {
            name: "com.example/http".to_string(),
            remotes: vec![RegistryRemote {
                kind: "streamable-http".to_string(),
                url: "https://example.com/mcp".to_string(),
                variables: Value::Null,
            }],
            ..Default::default()
        };
        assert_eq!(server_config(&server).unwrap(), json!({"type": "http", "url": "https://example.com/mcp"}));
    }

    #[test]
    fn rejects_entries_with_setup_variables() {
        let server = RegistryServer {
            name: "com.example/template".to_string(),
            remotes: vec![RegistryRemote {
                kind: "streamable-http".to_string(),
                url: "https://example.com/{region}/mcp".to_string(),
                variables: json!({"region": {"isRequired": true}}),
            }],
            ..Default::default()
        };
        assert!(server_config(&server).is_err());
    }

    #[test]
    fn converts_npm_stdio_for_windows() {
        let server = RegistryServer {
            name: "io.github/example".to_string(),
            packages: vec![RegistryPackage {
                registry_type: "npm".to_string(),
                identifier: "@example/mcp".to_string(),
                version: "1.2.3".to_string(),
                transport: RegistryTransport { kind: "stdio".to_string() },
                environment_variables: Vec::new(),
            }],
            ..Default::default()
        };
        let config = server_config(&server).unwrap();
        #[cfg(windows)]
        assert_eq!(config["command"], "cmd");
        #[cfg(not(windows))]
        assert_eq!(config["command"], "npx");
    }
}
