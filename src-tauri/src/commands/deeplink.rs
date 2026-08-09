//! Tauri commands for Deep Link / clipboard import preview + confirm.

use crate::database::dao;
use crate::database::dao::mcp as mcp_dao;
use crate::deeplink::{
    self, build_mcp_share_link, build_provider_share_link, import_result_label, mcp_inputs_from_preview,
    parse_import_text, provider_inputs_from_preview, DeeplinkImportResult, ImportPreview,
    ImportResource, McpExportEntry,
};
use crate::error::{AppError, AppResult};
use crate::mcp::{validate_server_input, McpServer};
use crate::provider::ProviderExportEntry;
use crate::store::AppState;

#[tauri::command]
pub fn preview_import_text(text: String) -> AppResult<ImportPreview> {
    parse_import_text(&text)
}

#[tauri::command]
pub fn confirm_import_preview(
    preview: ImportPreview,
    state: tauri::State<'_, AppState>,
) -> AppResult<DeeplinkImportResult> {
    match preview.resource {
        ImportResource::Provider => {
            let inputs = provider_inputs_from_preview(&preview)?;
            let mut imported = 0usize;
            let mut skipped = 0usize;
            let mut touched_opencode = false;
            for input in inputs {
                let existing = state
                    .db
                    .with_conn(|conn| dao::list_providers(conn, input.target_app))?;
                if existing.iter().any(|provider| {
                    provider.name == input.name && provider.base_url == input.base_url
                }) {
                    skipped += 1;
                    continue;
                }
                if input.target_app == crate::provider::ProviderTarget::OpenCode {
                    touched_opencode = true;
                }
                state.db.with_conn(|conn| dao::upsert_provider(conn, &input))?;
                imported += 1;
            }
            if touched_opencode {
                crate::commands::providers::sync_opencode_providers_to_live(&state)?;
            }
            Ok(import_result_label(ImportResource::Provider, imported, skipped))
        }
        ImportResource::Mcp => {
            let inputs = mcp_inputs_from_preview(&preview)?;
            let mut imported = 0usize;
            let mut skipped = 0usize;
            for input in inputs {
                validate_server_input(&input)?;
                let existing = state.db.with_conn(|conn| mcp_dao::list_mcp_servers(conn))?;
                if existing.iter().any(|server| server.name == input.name) {
                    skipped += 1;
                    continue;
                }
                state.db.with_conn(|conn| mcp_dao::upsert_mcp_server(conn, &input))?;
                imported += 1;
            }
            Ok(import_result_label(ImportResource::Mcp, imported, skipped))
        }
    }
}

#[tauri::command]
pub fn build_provider_deeplink(
    provider_id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let provider = state
        .db
        .with_conn(|conn| dao::get_provider(conn, &provider_id))?
        .ok_or_else(|| AppError::Config(format!("供应商不存在: {provider_id}")))?;
    build_provider_share_link(&ProviderExportEntry {
        name: provider.name,
        base_url: provider.base_url,
        model: provider.model,
        model_context_window: provider.model_context_window,
        web_search_enabled: provider.web_search_enabled,
        model_mapping: provider.model_mapping,
        protocol_type: provider.protocol_type,
        target_app: provider.target_app,
        notes: provider.notes,
        failover_group: provider.failover_group,
        failover_models: provider.failover_models,
    })
}

#[tauri::command]
pub fn build_mcp_deeplink(
    server_id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<String> {
    let server: McpServer = state
        .db
        .with_conn(|conn| mcp_dao::get_mcp_server(conn, &server_id))?
        .ok_or_else(|| AppError::Config(format!("MCP 不存在: {server_id}")))?;
    build_mcp_share_link(&McpExportEntry {
        name: server.name,
        server_config: server.server_config,
        enabled_claude_code: server.enabled_claude_code,
        enabled_claude_desktop: server.enabled_claude_desktop,
        enabled_codex: server.enabled_codex,
        enabled_opencode: server.enabled_opencode,
    })
}

/// Emit a parsed import preview (or error) to the frontend.
pub fn emit_deeplink_url<R: tauri::Runtime>(app: &tauri::AppHandle<R>, url: &str) {
    match parse_import_text(url) {
        Ok(preview) => {
            let _ = tauri::Emitter::emit(app, deeplink::DEEPLINK_IMPORT_EVENT, preview);
        }
        Err(error) => {
            let payload = serde_json::json!({ "message": error.to_string(), "url": url });
            let _ = tauri::Emitter::emit(app, deeplink::DEEPLINK_ERROR_EVENT, payload);
        }
    }
}

pub fn looks_like_deeplink(arg: &str) -> bool {
    arg.trim()
        .to_ascii_lowercase()
        .starts_with(deeplink::DEEPLINK_SCHEME)
}
