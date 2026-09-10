//! Generate TypeScript DTO bindings into `src/types/generated`.
//!
//! First batch: provider / gateway / usage models. Remaining IPC types stay
//! handwritten in `src/types/backend.ts` until later batches.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use ts_rs::TS;

    use crate::database::dao::gateway::{GatewayProfile, RouteMode, RouteRule};
    use crate::database::dao::proxy_logs::{
        CurrencyAmount, ModelPricing, PaginatedProxyLogs, ProxyLogFilters, ProxyRequestLog,
        UsageBreakdown, UsageSummary, UsageTrendPoint,
    };
    use crate::gateway::{RouteDecision, RouteSource};
    use crate::provider::{
        ClaudeModelMapping, ProtocolType, Provider, ProviderKind, ProviderTarget, ThinkingConfig,
    };

    fn generated_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/types/generated")
    }

    fn export_one<T: TS + 'static>(dir: &std::path::Path) {
        T::export_all_to(dir).expect("export binding");
    }

    #[test]
    fn export_bindings() {
        let dir = generated_dir();
        fs::create_dir_all(&dir).expect("create generated dir");
        for entry in fs::read_dir(&dir).expect("read generated dir").flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("ts") {
                let _ = fs::remove_file(path);
            }
        }

        export_one::<ProtocolType>(&dir);
        export_one::<ProviderTarget>(&dir);
        export_one::<ProviderKind>(&dir);
        export_one::<ThinkingConfig>(&dir);
        export_one::<ClaudeModelMapping>(&dir);
        export_one::<Provider>(&dir);
        export_one::<GatewayProfile>(&dir);
        export_one::<RouteMode>(&dir);
        export_one::<RouteRule>(&dir);
        export_one::<RouteSource>(&dir);
        export_one::<RouteDecision>(&dir);
        export_one::<CurrencyAmount>(&dir);
        export_one::<UsageSummary>(&dir);
        export_one::<UsageBreakdown>(&dir);
        export_one::<UsageTrendPoint>(&dir);
        export_one::<ModelPricing>(&dir);
        export_one::<ProxyRequestLog>(&dir);
        export_one::<PaginatedProxyLogs>(&dir);
        export_one::<ProxyLogFilters>(&dir);

        let mut exports: Vec<String> = fs::read_dir(&dir)
            .expect("list generated")
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let stem = path.file_stem()?.to_str()?.to_string();
                if path.extension()?.to_str()? == "ts" && stem != "index" {
                    Some(stem)
                } else {
                    None
                }
            })
            .collect();
        exports.sort();
        let barrel = exports
            .iter()
            .map(|name| format!("export * from \"./{name}\";"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(dir.join("index.ts"), format!("{barrel}\n")).expect("write barrel");

        // IPC numbers are JSON numbers; map ts-rs `bigint` (i64/u64) to TypeScript number.
        for entry in fs::read_dir(&dir).expect("rewrite generated").flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("ts") {
                continue;
            }
            let original = fs::read_to_string(&path).expect("read generated");
            let rewritten = original
                .replace("bigint", "number")
                .replace("{ [key in string]?: string }", "Record<string, string>");
            if rewritten != original {
                fs::write(path, rewritten).expect("rewrite bigint");
            }
        }
    }
}
