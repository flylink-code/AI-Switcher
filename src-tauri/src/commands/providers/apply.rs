
/// Try the standard model-list endpoint. Failure is non-fatal: providers that
/// do not expose it can still use a manually entered model name.
#[tauri::command]
pub async fn discover_provider_models(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ModelDiscoveryResult> {
    let provider = state.db.with_conn(|conn| {
        dao::get_provider(conn, &id)?.ok_or_else(|| AppError::Config(format!("供应商不存在: {id}")))
    })?;
    let key = state.db.with_conn(|conn| dao::resolve_api_key(conn, &provider.id))?;
    let Some(key) = key else {
        if uses_antigravity_model_catalog(&provider) {
            return discover_provider_models_with_key(&provider, String::new(), &state, true).await;
        }
        return cached_or_empty_model_result(
            &provider.id,
            "供应商未配置 API Key",
            &state,
        );
    };
    discover_provider_models_with_key(&provider, key, &state, true).await
}

#[tauri::command]
pub fn get_cached_provider_models(
    id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ModelDiscoveryResult> {
    state.db.with_conn(|conn| {
        if dao::get_provider(conn, &id)?.is_none() {
            return Err(AppError::Config(format!("供应商不存在: {id}")));
        }
        Ok(model_result_from_cache(
            dao::get_provider_model_cache(conn, &id)?,
            "已加载保存的模型列表",
            None,
        ))
    })
}

/// Discover models from an unsaved form without persisting its endpoint,
/// model, notes or newly entered credential.
#[tauri::command]
pub async fn discover_provider_models_input(
    input: ProviderInput,
    state: tauri::State<'_, AppState>,
) -> AppResult<ModelDiscoveryResult> {
    let provider = temporary_provider(&input, &state)?;
    discover_provider_models_with_key(&provider, provider.api_key.clone(), &state, false).await
}

pub(crate) async fn discover_provider_models_with_key(
    provider: &Provider,
    key: String,
    state: &AppState,
    cache_result: bool,
) -> AppResult<ModelDiscoveryResult> {
    let checked_at = Utc::now().timestamp_millis();

    // Built-in / local Antigravity gateway: serve the live Cloud Code catalog.
    // Avoid HTTP /v1/models against loopback — system proxies (Clash etc.) often
    // return HTTP 502 for 127.0.0.1, and the gateway may not be running yet while
    // the user is still filling the provider form.
    if uses_antigravity_model_catalog(provider) {
        let models = antigravity_catalog_model_ids();
        if models.is_empty() {
            let error = "Antigravity 暂无可用模型，请先在网关页登录账号并刷新额度".to_string();
            return if cache_result {
                cached_or_empty_model_result(&provider.id, &error, state)
            } else {
                Ok(ModelDiscoveryResult {
                    models: Vec::new(),
                    message: error.clone(),
                    checked_at,
                    source: "none".to_string(),
                    stale: false,
                    expires_at: None,
                    error: Some(error),
                })
            };
        }
        if cache_result {
            state.db.with_conn(|conn| {
                dao::save_provider_model_cache(conn, &provider.id, &models, checked_at)
            })?;
        }
        return Ok(ModelDiscoveryResult {
            models,
            message: "已从 Antigravity Cloud Code 目录加载模型".to_string(),
            checked_at,
            source: "antigravity".to_string(),
            stale: false,
            expires_at: cache_result.then_some(checked_at + MODEL_CACHE_TTL_MS),
            error: None,
        });
    }

    if provider.is_codex_oauth() {
        let account_id = provider.auth_binding.clone();
        let token_account = tauri::async_runtime::spawn_blocking(move || {
            crate::codex_oauth::manager().get_valid_token(Some(&account_id))
        })
        .await
        .map_err(|error| AppError::Tauri(format!("ChatGPT 模型发现任务失败: {error}")))?;
        let discovered = match token_account {
            Ok((token, account_id)) => fetch_codex_oauth_models(&token, &account_id).await,
            Err(error) => Err(error.to_string()),
        };
        return match discovered {
            Ok(models) => {
                if cache_result {
                    state.db.with_conn(|conn| {
                        dao::save_provider_model_cache(conn, &provider.id, &models, checked_at)
                    })?;
                }
                Ok(ModelDiscoveryResult {
                    models,
                    message: "已从 ChatGPT Codex 目录加载模型".to_string(),
                    checked_at,
                    source: "codex_oauth".to_string(),
                    stale: false,
                    expires_at: cache_result.then_some(checked_at + MODEL_CACHE_TTL_MS),
                    error: None,
                })
            }
            Err(error) if cache_result => cached_or_empty_model_result(&provider.id, &error, state),
            Err(error) => Ok(ModelDiscoveryResult {
                models: Vec::new(),
                message: error.clone(),
                checked_at,
                source: "none".to_string(),
                stale: false,
                expires_at: None,
                error: Some(error),
            }),
        };
    }

    let urls = model_discovery_urls(&provider.base_url)?;
    let client = discovery_http_client(urls.first().map(String::as_str).unwrap_or(""))?;
    let discovered = fetch_discovered_models(&client, &urls, &key, provider.custom_headers.as_ref()).await;
    match discovered {
        Ok(models) => {
            if cache_result {
                state.db.with_conn(|conn| {
                    dao::save_provider_model_cache(conn, &provider.id, &models, checked_at)
                })?;
            }
            Ok(ModelDiscoveryResult {
                models,
                message: "模型列表已更新".to_string(),
                checked_at,
                source: "network".to_string(),
                stale: false,
                expires_at: cache_result.then_some(checked_at + MODEL_CACHE_TTL_MS),
                error: None,
            })
        }
        Err(error) if cache_result => cached_or_empty_model_result(&provider.id, &error, state),
        Err(error) => Ok(ModelDiscoveryResult {
            models: Vec::new(),
            message: error.clone(),
            checked_at,
            source: "none".to_string(),
            stale: false,
            expires_at: None,
            error: Some(error),
        }),
    }
}

fn uses_antigravity_model_catalog(provider: &Provider) -> bool {
    provider.is_antigravity() || is_antigravity_gateway_base_url(&provider.base_url)
}

fn is_antigravity_gateway_base_url(base_url: &str) -> bool {
    let Ok(normalized) = normalize_base_url(base_url) else {
        return false;
    };
    let lower = normalized.to_ascii_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(lower.as_str());
    let host_port = without_scheme.split('/').next().unwrap_or("");
    matches!(
        host_port,
        "127.0.0.1:15830"
            | "localhost:15830"
            | "[::1]:15830"
            | "127.0.0.1:8045"
            | "localhost:8045"
            | "[::1]:8045"
    )
}

fn antigravity_catalog_model_ids() -> Vec<String> {
    // Seed from persisted account quotas when the in-memory catalog is cold.
    let _ = crate::antigravity::list_accounts();
    crate::antigravity::list_model_ids()
}

fn discovery_http_client(url: &str) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15));
    if url_targets_loopback(url) {
        // Clash/system proxy often 502s loopback API probes.
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|e| AppError::Other(format!("创建连接测试客户端失败: {e}")))
}

fn url_targets_loopback(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.contains("://127.0.0.1")
        || lower.contains("://localhost")
        || lower.contains("://[::1]")
}

fn cached_or_empty_model_result(
    provider_id: &str,
    error: &str,
    state: &AppState,
) -> AppResult<ModelDiscoveryResult> {
    state.db.with_conn(|conn| {
        Ok(model_result_from_cache(
            dao::get_provider_model_cache(conn, provider_id)?,
            "刷新失败，继续使用已保存的模型列表",
            Some(error.to_string()),
        ))
    })
}

fn model_result_from_cache(
    cache: Option<dao::providers::ProviderModelCache>,
    cached_message: &str,
    error: Option<String>,
) -> ModelDiscoveryResult {
    let now = Utc::now().timestamp_millis();
    match cache {
        Some(cache) if !cache.models.is_empty() => {
            let expires_at = cache.checked_at + MODEL_CACHE_TTL_MS;
            ModelDiscoveryResult {
                models: cache.models,
                message: cached_message.to_string(),
                checked_at: cache.checked_at,
                source: "cache".to_string(),
                stale: now >= expires_at,
                expires_at: Some(expires_at),
                error,
            }
        }
        _ => {
            let message = error
                .clone()
                .unwrap_or_else(|| "尚未保存模型列表".to_string());
            ModelDiscoveryResult {
                models: Vec::new(),
                message,
                checked_at: now,
                source: "none".to_string(),
                stale: false,
                expires_at: None,
                error,
            }
        }
    }
}

/// Candidate model-list URLs. DeepSeek's official list is `GET /models` on the
/// host root; Anthropic-compat bases (`.../anthropic`) 404 on `/v1/models`.
fn model_discovery_urls(base_url: &str) -> AppResult<Vec<String>> {
    let mut urls = Vec::new();
    let mut push = |url: String| {
        if !url.is_empty() && !urls.iter().any(|existing| existing == &url) {
            urls.push(url);
        }
    };
    push(api_endpoint_url(base_url, "/v1/models")?);
    push(api_endpoint_url(base_url, "/models")?);

    let base = normalize_base_url(base_url)?;
    if let Some(root) = base.strip_suffix("/v1") {
        if root.contains("://") {
            push(format!("{root}/models"));
        }
    }
    if let Some(root) = strip_anthropic_compat_path(&base) {
        push(api_endpoint_url(&root, "/v1/models")?);
        push(api_endpoint_url(&root, "/models")?);
    }
    Ok(urls)
}

fn should_try_next_discovery_status(status: u16) -> bool {
    matches!(status, 404 | 405 | 501)
}

async fn fetch_codex_oauth_models(token: &str, account_id: &str) -> Result<Vec<String>, String> {
    let client = discovery_http_client(crate::codex_oauth::CODEX_OAUTH_MODELS_URL)
        .map_err(|error| error.to_string())?;
    let response = client
        .get(crate::codex_oauth::build_codex_oauth_models_url())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("originator", crate::codex_oauth::ORIGINATOR)
        .header("version", crate::codex_oauth::CLIENT_VERSION)
        .header("chatgpt-account-id", account_id)
        .send()
        .await
        .map_err(|error| format!("Request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let body = if body.chars().count() > 512 {
            format!("{}...", body.chars().take(512).collect::<String>())
        } else {
            body
        };
        return Err(format!("HTTP {status}: {body}"));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("Failed to parse response: {error}"))?;
    let models = crate::codex_oauth::parse_codex_oauth_model_ids(&value);
    if models.is_empty() {
        Err("ChatGPT 没有返回可用的模型".to_string())
    } else {
        Ok(models)
    }
}

async fn fetch_discovered_models(
    client: &reqwest::Client,
    urls: &[String],
    key: &str,
    custom_headers: Option<&std::collections::HashMap<String, String>>,
) -> Result<Vec<String>, String> {
    let mut last_error = "无法连接模型发现端点".to_string();
    for url in urls {
        let mut request = client
            .get(url)
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
        if let Some(headers) = custom_headers {
            for (k, v) in headers {
                if !crate::proxy::is_hop_by_hop_header(k)
                    && !k.eq_ignore_ascii_case("host")
                    && !k.eq_ignore_ascii_case("content-length")
                {
                    request = request.header(k.as_str(), v.as_str());
                }
            }
        }
        let response = request.send().await;
        match response {
            Ok(response) if response.status().is_success() => {
                let parsed = match response.bytes().await {
                    Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                        Ok(value) => {
                            let models = extract_model_ids(&value);
                            if models.is_empty() {
                                Err("供应商没有返回可用的模型".to_string())
                            } else {
                                Ok(models)
                            }
                        }
                        Err(_) => Err("模型发现响应不是有效 JSON".to_string()),
                    },
                    Err(_) => Err("读取模型发现响应失败".to_string()),
                };
                match parsed {
                    Ok(models) => return Ok(models),
                    Err(error) => {
                        last_error = error;
                        continue;
                    }
                }
            }
            Ok(response) => {
                let status = response.status().as_u16();
                last_error = format!("供应商不支持模型发现（HTTP {status}）");
                if should_try_next_discovery_status(status) {
                    continue;
                }
                return Err(last_error);
            }
            Err(_) => {
                last_error = "无法连接模型发现端点".to_string();
            }
        }
    }
    Err(last_error)
}

fn extract_model_ids(value: &Value) -> Vec<String> {
    let mut models = BTreeSet::new();
    if let Some(items) = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
    {
        for item in items {
            let value = item.as_str().or_else(|| {
                item.get("id")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
            });
            let Some(model) = value.map(str::trim) else {
                continue;
            };
            if !model.is_empty() && model.chars().count() <= MAX_MODEL_NAME_CHARS {
                models.insert(model.to_string());
                if models.len() >= MAX_DISCOVERED_MODELS {
                    break;
                }
            }
        }
    }
    models.into_iter().collect()
}

#[tauri::command]
pub async fn switch_to_official(
    target: ProviderTarget,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<()> {
    switch_to_official_for_target(target, Some(&app), &state).await
}

/// Shared official-login restoration used by IPC and tray actions.
pub async fn switch_to_official_for_target<R: tauri::Runtime>(
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
) -> AppResult<()> {
    restore_official_for_target(target, app, state, true).await
}

async fn restore_official_for_target<R: tauri::Runtime>(
    target: ProviderTarget,
    app: Option<&tauri::AppHandle<R>>,
    state: &AppState,
    clear_gateway_catalog: bool,
) -> AppResult<()> {
    let mut snapshot = SwitchSnapshot::capture(state, target).await?;
    let result: AppResult<()> = async {
        match target {
            ProviderTarget::ClaudeCode => restore_code_ownership(state)?,
            ProviderTarget::ClaudeDesktop => restore_desktop_ownership(state)?,
            ProviderTarget::Codex => {
                tauri::async_runtime::spawn_blocking(codex::restore_official)
                    .await
                    .map_err(|error| {
                        AppError::Tauri(format!("Codex 官方配置恢复任务失败: {error}"))
                    })??;
            }
            ProviderTarget::OpenCode => {
                tauri::async_runtime::spawn_blocking(opencode::clear_provider)
                    .await
                    .map_err(|error| {
                        AppError::Tauri(format!("OpenCode 托管配置移除任务失败: {error}"))
                    })??;
            }
            // Pi / Dsh 无独立「官方配置」文件；清除当前供应商 + 停代理即可。
            ProviderTarget::Pi | ProviderTarget::Dsh | ProviderTarget::Cline => {}
        }
        state.proxy.lock().await.stop_target(target);
        if let Some(app) = app {
            crate::commands::proxy::publish_target_stopped(app, state, target).await;
        }
        state.db.with_conn(|conn| {
            dao::clear_current_provider(conn, target)?;
            crate::database::dao::gateway::set_current_connection_type(
                conn,
                target.as_str(),
                crate::database::dao::gateway::ConnectionType::External,
            )?;
            if clear_gateway_catalog {
                if let Some(key) = catalog::setting_key(target) {
                    set_setting(conn, key, "false")?;
                }
            }
            Ok(())
        })?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            let _ = snapshot.capture_last_written_files();
            Ok(())
        }
        Err(error) => {
            if let Err(mark_error) = snapshot.capture_last_written_files() {
                return rollback_switch(
                    snapshot,
                    state,
                    AppError::Config(format!(
                        "{error}；无法安全确认配置写入状态：{mark_error}"
                    )),
                )
                .await;
            }
            rollback_switch(snapshot, state, error).await
        }
    }
}
