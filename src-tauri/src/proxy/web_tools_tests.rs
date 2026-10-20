#[test]
fn search_results_keep_links_and_apply_domain_boundaries() {
    let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fcode.visualstudio.com%2Fdocs">工作区 &amp; Trust</a>
        <a class="result__a" href="https://evilvisualstudio.com/docs">Wrong domain</a>
        <a class="result__a" href="https://blocked.visualstudio.com/docs">Blocked</a>"#;
    let results = parse_search_results(html, &json!({
        "allowed_domains":["visualstudio.com"], "blocked_domains":["blocked.visualstudio.com"]
    }));
    assert_eq!(results.as_array().unwrap().len(), 1);
    assert_eq!(results[0]["url"], "https://code.visualstudio.com/docs");
    assert_eq!(results[0]["title"], "工作区 & Trust");
    assert_eq!(results[0]["type"], "web_search_result");
}

#[test]
fn challenge_is_not_a_successful_empty_search() {
    assert_eq!(parse_search_results("<form>Verify you are human</form>", &json!({}))["error_code"], "unavailable");
    assert_eq!(parse_search_results("<div class=\"no-results\">No results</div>", &json!({})), json!([]));
    assert_eq!(clip("工作区", 4), "工…");
}

#[tokio::test]
async fn server_search_errors_have_linked_result_and_terminal_reason() {
    let tools = server_web_tools(&json!({"tools":[{"type":"web_search_20250305","name":"web_search"}]}));
    let mut response = json!({"content":[{"type":"tool_use","id":"search_1","name":"web_search","input":{"query":""}}],
        "stop_reason":"tool_use"});
    assert!(materialize_web_tool_uses(&mut response, &tools).await);
    assert_eq!(response["content"][0]["type"], "server_tool_use");
    assert_eq!(response["content"][1]["type"], "web_search_tool_result");
    assert_eq!(response["content"][1]["tool_use_id"], "search_1");
    assert_eq!(response["content"][1]["content"]["error_code"], "invalid_input");
    assert_eq!(response["stop_reason"], "end_turn");
}

#[tokio::test]
async fn custom_search_and_native_server_results_are_not_executed_again() {
    let mut response = json!({"content":[{"type":"tool_use","id":"custom","name":"web_search","input":{}}],
        "stop_reason":"tool_use"});
    let original = response.clone();
    assert!(!materialize_web_tool_uses(&mut response, &[]).await);
    assert_eq!(response, original);
    let tools = server_web_tools(&json!({"tools":[{"type":"web_search_20250305","name":"web_search"}]}));
    let mut response = json!({"content":[{"type":"server_tool_use","id":"native","name":"web_search","input":{}},
        {"type":"web_search_tool_result","tool_use_id":"native","content":[]}],"stop_reason":"end_turn"});
    let original = response.clone();
    assert!(!materialize_web_tool_uses(&mut response, &tools).await);
    assert_eq!(response, original);
}

#[tokio::test]
async fn streaming_search_is_materialized_with_complete_split_input() {
    let bytes = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"type\":\"message\",\"id\":\"m1\",\"role\":\"assistant\",\"usage\":{\"input_tokens\":5}}}\r\n\r\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"s1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );
    let response = axum::response::Response::builder().body(axum::body::Body::from(bytes)).unwrap();
    let tools = vec![json!({"type":"web_search_20250305","name":"web_search"})];
    let response = materialize_stream(response, &tools).await;
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let result = collect_message(&bytes).unwrap();
    assert_eq!(result["content"][0]["input"], json!({"query":""}));
    assert_eq!(result["content"][1]["content"]["error_code"], "invalid_input");
    assert_eq!(result["usage"]["input_tokens"], 5);
    assert_eq!(result["usage"]["output_tokens"], 7);
    assert_eq!(result["stop_reason"], "end_turn");
}

#[test]
fn incomplete_or_failed_streams_are_not_fabricated_as_success() {
    assert!(collect_message(b"data: {\"type\":\"message_start\",\"message\":{}}\n\n").is_none());
    assert!(collect_message(b"data: {\"type\":\"error\",\"error\":{\"message\":\"failed\"}}\n\n").is_none());
}

#[tokio::test]
#[ignore = "requires local gateway credentials and real Claude CLI; consumes upstream usage"]
async fn live_cli_network_gateway() {
    use axum::{routing::post, Router};
    let key = std::env::var("AISW_CONTEXT_TEST_KEY").expect("isolated gateway credential");
    let credential = crate::secrets::test_credentials::Credential::new(&key);
    let mut state = super::super::tests::circuit_test_state();
    state.target = crate::provider::ProviderTarget::VscodeCopilot;
    state.client = reqwest::Client::builder().no_proxy().build().unwrap();
    state.db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO providers (id,name,base_url,api_key,model,is_current,target_app,protocol_type)
             VALUES ('network-live','network-live','http://127.0.0.1:15828',?1,'claude-sonnet-5.5',1,?2,'anthropic')",
            rusqlite::params![credential.reference(), state.target.as_str()],
        )?;
        Ok(())
    }).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:15840").await.unwrap();
    println!("NETWORK_TEST_READY 15840");
    axum::serve(listener, Router::new().route("/v1/messages", post(super::super::proxy_handler)).with_state(state))
        .with_graceful_shutdown(async { tokio::time::sleep(std::time::Duration::from_secs(100)).await; })
        .await.unwrap();
}
