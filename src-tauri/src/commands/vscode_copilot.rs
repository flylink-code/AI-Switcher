//! VS Code Copilot 的 Agent 页命令：绑定独立 token、写入/移除/校验托管的
//! `chatLanguageModels.json` 条目。请求仍走智能网关，不启动本地代理。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::vscode_copilot::{
    apply_managed_at, chat_language_models_path, diff_managed, managed_entry, read_managed_at,
    remove_managed_at, CopilotModel, DriftField,
};
use crate::database::dao::gateway as gw;
use crate::error::{AppError, AppResult};
use crate::provider::ProviderTarget;
use crate::store::AppState;

const TARGET: ProviderTarget = ProviderTarget::VscodeCopilot;
const OWNERSHIP_KEY: &str = "v1400.vscode_copilot_managed";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VscodeCopilotStatus {
    pub config_path: String,
    pub file_exists: bool,
    pub bound: bool,
    pub applied: bool,
    pub mode: String,
    pub direct_upstream_id: Option<String>,
    pub model_count: usize,
    pub gateway_running: bool,
    pub drift: Vec<DriftField>,
}

fn gateway_base_url(state: &AppState) -> String {
    let port = state
        .db
        .with_read_conn(|conn| {
            crate::database::dao::settings::get_setting(conn, crate::gateway::service::PORT_SETTING)
        })
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(crate::gateway::SMART_GATEWAY_PORT);
    format!("http://127.0.0.1:{port}/v1")
}

/// 按 Copilot 身份的可见目录构造 `models[]`。元数据优先级：库内保存值 > 推断 > 默认。
fn build_models(state: &AppState) -> AppResult<Vec<CopilotModel>> {
    state.db.with_read_conn(|conn| {
        let binding = gw::binding_for_target(conn, TARGET)?;
        let mut providers = gw::list_upstream_providers(conn, false)?;
        providers.retain(|provider| !provider.is_smart_gateway());
        if let Some(b) = binding.as_ref().filter(|b| b.mode == "direct") {
            let direct_id = b.direct_upstream_id.trim();
            if !direct_id.is_empty() {
                providers.retain(|p| p.id == direct_id);
            }
        } else if let Ok(Some(profile)) = gw::current_profile(conn, TARGET) {
            if !profile.allowed_upstream_ids.is_empty() {
                providers.retain(|p| gw::profile_allows_upstream(&profile, &p.id));
            }
        }
        let hide_official = crate::catalog::hide_official_for_conn(conn, TARGET);
        let mut pairs = Vec::with_capacity(providers.len());
        let mut meta: HashMap<(String, String), gw::GatewayUpstreamModelRow> = HashMap::new();
        for provider in &providers {
            let rows = gw::list_upstream_models(conn, &provider.id).unwrap_or_default();
            let visible: Vec<String> = rows
                .iter()
                .filter(|row| row.visible)
                .map(|row| row.model_id.clone())
                .collect();
            for row in rows {
                meta.insert((provider.id.clone(), row.model_id.clone()), row);
            }
            pairs.push((provider.clone(), visible));
        }
        let style = crate::catalog::catalog_style_for(TARGET);
        let entries = crate::catalog::build_catalog_with(style, &pairs, hide_official);
        Ok(entries
            .into_iter()
            .map(|entry| {
                let slug = if entry.upstream_slug.trim().is_empty() {
                    entry.public_id.clone()
                } else {
                    entry.upstream_slug.clone()
                };
                let row = meta.get(&(entry.provider_id.clone(), slug.clone()));
                let inferred = crate::gateway::metadata::infer(&slug);
                let capability = |key: &str, fallback: bool| -> bool {
                    row.and_then(|r| r.capabilities.get(key).and_then(|v| v.as_bool()))
                        .or_else(|| inferred.capabilities.get(key).and_then(|v| v.as_bool()))
                        .unwrap_or(fallback)
                };
                let context = row
                    .and_then(|r| r.context_window)
                    .filter(|v| *v > 0)
                    .map(|v| v as u64)
                    .unwrap_or(entry.context_window)
                    .max(1);
                let max_output = row
                    .and_then(|r| r.max_output_tokens)
                    .filter(|v| *v > 0)
                    .map(|v| v as u64)
                    .unwrap_or_else(|| crate::gateway::metadata::max_output_for(&slug, context))
                    .min(context.saturating_sub(1).max(1));
                let mut efforts: Vec<String> = row
                    .map(|r| r.reasoning_levels.clone())
                    .filter(|levels| !levels.is_empty())
                    .unwrap_or_else(|| inferred.reasoning_levels.clone())
                    .into_iter()
                    .filter(|level| matches!(level.as_str(), "minimal" | "low" | "medium" | "high"))
                    .collect();
                // 升级旧版库内记录：Gemini 系列模型（如 gemini-3.8-flash / gemini-4-argon）
                // 早期被推断为仅含 ["off", "low"]，补齐完整的 low/medium/high 推理档位。
                if slug.to_ascii_lowercase().contains("gemini") && efforts == vec!["low".to_string()] {
                    efforts = inferred
                        .reasoning_levels
                        .into_iter()
                        .filter(|level| matches!(level.as_str(), "minimal" | "low" | "medium" | "high"))
                        .collect();
                }
                // 仅在多供应商同名冲突（public_id 被加了供应商前缀，即 public_id != slug）时保留
                // 供应商前缀 `entry.display_name`；单一来源模型直接使用干净的模型名，避免在
                // VS Code Copilot 的 AI-Switcher 分组下额外显示 `Antigravity (Built-in) · ` 等反代前缀。
                let is_shared = !entry.public_id.eq_ignore_ascii_case(&slug);
                let default_name = if is_shared {
                    entry.display_name.clone()
                } else {
                    slug.clone()
                };
                let name = row
                    .map(|r| r.display_name.trim())
                    .filter(|n| !n.is_empty() && *n != entry.display_name)
                    .map(|n| crate::catalog::strip_client_model_prefix(n).to_string())
                    .filter(|n| !n.is_empty())
                    .unwrap_or(default_name);
                CopilotModel {
                    id: entry.public_id,
                    name,
                    // Copilot Agent 模式必须能调用工具。
                    tool_calling: capability("tools", true),
                    vision: capability("vision", false),
                    thinking: !efforts.is_empty(),
                    reasoning_efforts: efforts,
                    context_window: context,
                    max_input_tokens: context.saturating_sub(max_output).max(1),
                    max_output_tokens: max_output,
                }
            })
            .collect())
    })
}

fn expected_entry(state: &AppState) -> AppResult<(serde_json::Value, usize)> {
    let binding = state
        .db
        .with_read_conn(|conn| gw::binding_for_target(conn, TARGET))?
        .ok_or_else(|| AppError::Config("VS Code Copilot 尚未绑定智能网关".into()))?;
    let models = build_models(state)?;
    let count = models.len();
    Ok((
        managed_entry(&gateway_base_url(state), &binding.entry_token, &models),
        count,
    ))
}

/// 刷新托管条目。未绑定时什么也不做；被模型/供应商变更事件复用。
pub(crate) fn sync_managed_config(state: &AppState) -> AppResult<()> {
    let bound = state
        .db
        .with_read_conn(|conn| gw::binding_for_target(conn, TARGET))?
        .is_some();
    if !bound {
        return Ok(());
    }
    let (entry, _) = expected_entry(state)?;
    apply_managed_at(&chat_language_models_path(), entry)
}

struct CopilotMutationSnapshot {
    path: PathBuf,
    file_contents: Option<Vec<u8>>,
    binding: Option<gw::GatewayBinding>,
    ownership: Option<String>,
}

impl CopilotMutationSnapshot {
    fn capture(state: &AppState, path: &Path) -> AppResult<Self> {
        let file_contents = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(AppError::Io(format!("读取 {} 失败: {err}", path.display()))),
        };
        let (binding, ownership) = state.db.with_conn(|conn| {
            Ok((
                gw::binding_for_target(conn, TARGET)?,
                crate::database::dao::settings::get_setting(conn, OWNERSHIP_KEY)?,
            ))
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            file_contents,
            binding,
            ownership,
        })
    }

    fn restore(&self, state: &AppState) -> AppResult<()> {
        let mut failures = Vec::new();
        let file_res = match &self.file_contents {
            Some(bytes) => crate::config::atomic_write(&self.path, bytes),
            None => match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(AppError::Io(format!(
                    "移除未完成配置文件失败 ({}): {err}",
                    self.path.display()
                ))),
            },
        };
        if let Err(err) = file_res {
            failures.push(format!("恢复配置文件失败: {err}"));
        }
        let db_res = state.db.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM gateway_bindings WHERE target_app = ?1",
                [TARGET.as_str()],
            )?;
            if let Some(binding) = &self.binding {
                tx.execute(
                    "INSERT INTO gateway_bindings (target_app,entry_token,provider_id,created_at,profile_id,mode,direct_upstream_id) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![
                        TARGET.as_str(),
                        binding.entry_token,
                        binding.provider_id,
                        binding.created_at,
                        binding.profile_id,
                        binding.mode,
                        binding.direct_upstream_id,
                    ],
                )?;
            }
            crate::database::dao::settings::set_setting(
                &tx,
                OWNERSHIP_KEY,
                self.ownership.as_deref().unwrap_or(""),
            )?;
            tx.commit()?;
            Ok(())
        });
        if let Err(err) = db_res {
            failures.push(format!("恢复数据库绑定失败: {err}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Config(failures.join("；")))
        }
    }
}

fn apply_managed_with_rollback(state: &AppState, path: &Path) -> AppResult<()> {
    let snapshot = CopilotMutationSnapshot::capture(state, path)?;
    let result = (|| -> AppResult<()> {
        state
            .db
            .with_conn(|conn| gw::set_gateway_binding(conn, TARGET).map(|_| ()))?;
        let (entry, _) = expected_entry(state)?;
        apply_managed_at(path, entry)?;
        state.db.with_conn(|conn| {
            crate::database::dao::settings::set_setting(conn, OWNERSHIP_KEY, "1")
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(rollback_error) = snapshot.restore(state) {
            return Err(AppError::Config(format!(
                "{error}（回滚失败：{rollback_error}）"
            )));
        }
        return Err(error);
    }
    Ok(())
}

fn remove_managed_with_rollback(state: &AppState, path: &Path) -> AppResult<()> {
    let snapshot = CopilotMutationSnapshot::capture(state, path)?;
    let result = (|| -> AppResult<()> {
        remove_managed_at(path)?;
        state.db.with_conn(|conn| {
            gw::delete_binding(conn, TARGET)?;
            crate::database::dao::settings::set_setting(conn, OWNERSHIP_KEY, "")
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(rollback_error) = snapshot.restore(state) {
            return Err(AppError::Config(format!(
                "{error}（回滚失败：{rollback_error}）"
            )));
        }
        return Err(error);
    }
    Ok(())
}

fn status_at(state: &AppState, path: &Path) -> AppResult<VscodeCopilotStatus> {
    let binding = state
        .db
        .with_read_conn(|conn| gw::binding_for_target(conn, TARGET))?;
    let bound = binding.is_some();
    let mode = match binding.as_ref() {
        Some(b) if b.mode == "direct" => "direct".to_string(),
        Some(_) => "gateway".to_string(),
        None => "official".to_string(),
    };
    let direct_upstream_id = binding
        .as_ref()
        .filter(|b| b.mode == "direct" && !b.direct_upstream_id.trim().is_empty())
        .map(|b| b.direct_upstream_id.clone());
    let current = read_managed_at(path)?;
    let (drift, model_count) = if bound {
        let (expected, count) = expected_entry(state)?;
        (
            diff_managed(&expected, current.as_ref()),
            if current.is_some() { count } else { 0 },
        )
    } else {
        (Vec::new(), 0)
    };
    Ok(VscodeCopilotStatus {
        config_path: path.display().to_string(),
        file_exists: path.exists(),
        bound,
        applied: current.is_some(),
        mode,
        direct_upstream_id,
        model_count,
        gateway_running: crate::gateway::service::current_status().running,
        drift,
    })
}

fn status_inner(state: &AppState) -> AppResult<VscodeCopilotStatus> {
    status_at(state, &chat_language_models_path())
}

#[tauri::command]
pub fn get_vscode_copilot_status(
    state: tauri::State<'_, AppState>,
) -> AppResult<VscodeCopilotStatus> {
    status_inner(&state)
}

/// 绑定独立 token、确保网关运行，并写入托管条目。
#[tauri::command]
pub async fn apply_vscode_copilot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<VscodeCopilotStatus> {
    let _guard = crate::commands::providers::agent_connection_lock()
        .lock()
        .await;
    let mut enabled = state
        .db
        .with_conn(|conn| gw::list_upstream_providers(conn, false))?;
    enabled.retain(|provider| !provider.is_smart_gateway());
    if enabled.is_empty() {
        return Err(AppError::Config("请先添加至少一个供应商".into()));
    }
    crate::commands::providers::ensure_reverse_gateways_for_pool(&state).await?;
    crate::gateway::service::mark_enabled(state.db.as_ref())?;
    if !crate::gateway::service::current_status().running {
        crate::gateway::service::start_via_state(&state, None).await?;
    }
    apply_managed_with_rollback(&state, &chat_language_models_path())?;
    crate::catalog::invalidate_view_cache();
    crate::gateway::service::emit_status(&app);
    status_inner(&state)
}

fn apply_direct_with_rollback(state: &AppState, path: &Path, upstream_id: &str) -> AppResult<()> {
    let snapshot = CopilotMutationSnapshot::capture(state, path)?;
    let result = (|| -> AppResult<()> {
        state.db.with_conn(|conn| {
            gw::set_direct_binding(conn, TARGET, upstream_id).map(|_| ())
        })?;
        let (entry, _) = expected_entry(state)?;
        apply_managed_at(path, entry)?;
        state.db.with_conn(|conn| {
            crate::database::dao::settings::set_setting(conn, OWNERSHIP_KEY, "1")
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(rollback_error) = snapshot.restore(state) {
            return Err(AppError::Config(format!(
                "{error}（回滚失败：{rollback_error}）"
            )));
        }
        return Err(error);
    }
    Ok(())
}

/// 绑定指定单个上游（直连模式），仅将该上游的模型写入 `chatLanguageModels.json`。
#[tauri::command]
pub async fn set_vscode_copilot_direct(
    upstream_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<VscodeCopilotStatus> {
    let _guard = crate::commands::providers::agent_connection_lock()
        .lock()
        .await;
    let trimmed = upstream_id.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("请选择要直连的上游供应商".into()));
    }
    let exists = state.db.with_read_conn(|conn| {
        Ok(gw::list_upstream_providers(conn, false)?
            .into_iter()
            .any(|p| p.id == trimmed && !p.is_smart_gateway()))
    })?;
    if !exists {
        return Err(AppError::Config(format!("未找到启用中的上游供应商: {trimmed}")));
    }
    crate::commands::providers::ensure_reverse_gateways_for_pool(&state).await?;
    crate::gateway::service::mark_enabled(state.db.as_ref())?;
    if !crate::gateway::service::current_status().running {
        crate::gateway::service::start_via_state(&state, None).await?;
    }
    apply_direct_with_rollback(&state, &chat_language_models_path(), trimmed)?;
    crate::catalog::invalidate_view_cache();
    crate::gateway::service::emit_status(&app);
    status_inner(&state)
}

/// 移除托管条目并解除绑定；用户其他 provider 保留。
#[tauri::command]
pub async fn remove_vscode_copilot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<VscodeCopilotStatus> {
    let _guard = crate::commands::providers::agent_connection_lock()
        .lock()
        .await;
    remove_managed_with_rollback(&state, &chat_language_models_path())?;
    crate::catalog::invalidate_view_cache();
    crate::gateway::service::emit_status(&app);
    status_inner(&state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn test_state() -> AppState {
        let db = Arc::new(Database::memory().expect("in-memory db"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        AppState {
            db: Arc::clone(&db),
            proxy: tokio::sync::Mutex::new(crate::proxy::ProxyManager::new(db, tx)),
            proxy_status: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    #[test]
    fn status_surfaces_corrupted_chat_language_models_file() {
        let state = test_state();
        let dir = tempdir().unwrap();
        let path = dir.path().join("chatLanguageModels.json");
        std::fs::write(&path, r#"{"invalid_root": true}"#).unwrap();

        let err = status_at(&state, &path).expect_err("non-array root must surface error");
        assert!(err.to_string().contains("根节点必须是数组"));
    }

    #[test]
    fn apply_rolls_back_db_binding_when_config_write_fails() {
        let state = test_state();
        let dir = tempdir().unwrap();
        let path = dir.path().join("chatLanguageModels.json");
        std::fs::write(&path, r#"{"not": "an array"}"#).unwrap();

        let err = apply_managed_with_rollback(&state, &path)
            .expect_err("corrupted config file must fail apply");
        assert!(err.to_string().contains("根节点必须是数组"));

        let binding = state
            .db
            .with_read_conn(|conn| gw::binding_for_target(conn, TARGET))
            .unwrap();
        assert!(
            binding.is_none(),
            "binding must be rolled back on file write failure"
        );
        let ownership = state
            .db
            .with_read_conn(|conn| crate::database::dao::settings::get_setting(conn, OWNERSHIP_KEY))
            .unwrap();
        assert!(ownership.unwrap_or_default().is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"not": "an array"}"#
        );
    }

    #[test]
    fn build_models_strips_single_source_provider_prefix_and_upgrades_gemini_efforts() {
        let state = test_state();
        state
            .db
            .with_conn(|conn| {
                let ag = gw::upsert_upstream(
                    conn,
                    &crate::provider::ProviderInput {
                        id: Some("up_ag".into()),
                        name: "Antigravity (Built-in)".into(),
                        base_url: "http://127.0.0.1:15830".into(),
                        api_key: String::new(),
                        clear_api_key: false,
                        model: "gemini-3.8-flash-high".into(),
                        model_context_window: None,
                        auto_review_model_override: None,
                        web_search_enabled: None,
                        model_mapping: crate::provider::ClaudeModelMapping::default(),
                        protocol_type: crate::provider::ProtocolType::Anthropic,
                        provider_kind: crate::provider::ProviderKind::Antigravity,
                        auth_binding: String::new(),
                        target_app: crate::provider::ProviderTarget::ClaudeCode,
                        notes: String::new(),
                        failover_group: 0,
                        failover_models: vec!["gemini-4-argon-high".into()],
                        hidden_models: Vec::new(),
                        thinking_config: None,
                        custom_headers: None,
                    },
                )?;
                // 模拟旧版 upstream_models 里存了 ["off", "low"] 与带前缀的 display_name。
                conn.execute(
                    "INSERT INTO upstream_models (id, upstream_id, model_id, verified_status, visible, display_name, context_window, max_output_tokens, reasoning_levels_json, capabilities_json)
                     VALUES (?1, ?2, 'gemini-3.8-flash-high', 'declared', 1, 'Antigravity (Built-in) · gemini-3.8-flash-high', 1000000, 65536, '[\"off\",\"low\"]', '{}');",
                    rusqlite::params![format!("{}:gemini-3.8-flash-high", ag.id), ag.id],
                )?;
                gw::set_gateway_binding(conn, TARGET)?;
                Ok(())
            })
            .unwrap();

        let models = build_models(&state).unwrap();
        let flash = models
            .iter()
            .find(|m| m.id == "gemini-3.8-flash-high")
            .expect("gemini-3.8-flash-high in models");
        assert_eq!(flash.name, "gemini-3.8-flash-high");
        assert_eq!(
            flash.reasoning_efforts,
            vec!["low".to_string(), "medium".to_string(), "high".to_string()]
        );

        let argon = models
            .iter()
            .find(|m| m.id == "gemini-4-argon-high")
            .expect("gemini-4-argon-high in models");
        assert_eq!(argon.name, "gemini-4-argon-high");
        assert_eq!(
            argon.reasoning_efforts,
            vec!["low".to_string(), "medium".to_string(), "high".to_string()]
        );
    }
}
