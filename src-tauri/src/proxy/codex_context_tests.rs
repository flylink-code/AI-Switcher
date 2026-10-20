// Regression tests included in codex::tests.

#[tokio::test]
async fn response_cache_observes_fragmented_sse_without_changing_stream() {
    let history = Arc::new(super::super::codex_history::CodexHistoryStore::default());
    let event = concat!(
        "event: response.completed\r\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_sse\",",
        "\"output\":[{\"type\":\"message\",\"role\":\"assistant\",",
        "\"content\":[{\"type\":\"output_text\",\"text\":\"已记住\"}]}]}}\r\n\r\n"
    );
    let chunks: Vec<_> = event
        .as_bytes()
        .chunks(7)
        .map(|chunk| Ok::<Bytes, Infallible>(Bytes::copy_from_slice(chunk)))
        .collect();
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream::iter(chunks)))
        .unwrap();
    let response = cache_responses_exchange(
        response,
        Arc::clone(&history),
        json!({"input":"Remember BLUE742"}),
    )
    .await;
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), event.as_bytes());
    let mut next = json!({"previous_response_id":"resp_sse","input":"recall"});
    history.restore_conversation(&mut next).unwrap();
    assert_eq!(next["input"][0]["content"], "Remember BLUE742");
    assert_eq!(next["input"][1]["content"][0]["text"], "已记住");
}

#[tokio::test]
async fn response_cache_observes_json_and_does_not_cache_failed_sse() {
    let history = Arc::new(super::super::codex_history::CodexHistoryStore::default());
    let body = json!({"id":"resp_json","output":[{"role":"assistant","content":"OK"}]}).to_string();
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.clone()))
        .unwrap();
    let response =
        cache_responses_exchange(response, Arc::clone(&history), json!({"input":"BLUE742"})).await;
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), body.as_bytes());
    let mut next = json!({"previous_response_id":"resp_json","input":"recall"});
    history.restore_conversation(&mut next).unwrap();
    assert_eq!(next["input"][0]["content"], "BLUE742");

    let failed = "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_failed\",\"output\":[]}}\n\n";
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(failed))
        .unwrap();
    let response = cache_responses_exchange(
        response,
        Arc::clone(&history),
        json!({"input":"failed request"}),
    )
    .await;
    let _ = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let mut next = json!({"previous_response_id":"resp_failed","input":"recall"});
    assert!(history.restore_conversation(&mut next).is_err());
}

#[tokio::test]
async fn responses_handler_restores_two_rounds_for_anthropic_json_and_sse() {
    for streaming in [false, true] {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let captured = Arc::clone(&seen);
        let mock = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let captured = Arc::clone(&captured);
                async move {
                    captured.lock().unwrap().push(body.clone());
                    let mut reply = crate::kiro::map::CollectedReply::default();
                    reply.text = "已记住".to_string();
                    if body["stream"] == true {
                        Response::builder()
                            .header(header::CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(crate::kiro::map::anthropic_sse(
                                "claude-sonnet-5.5",
                                &reply,
                            )))
                            .unwrap()
                    } else {
                        axum::Json(crate::kiro::map::anthropic_message(
                            "claude-sonnet-5.5",
                            &reply,
                        ))
                        .into_response()
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
        let mut state = super::super::tests::circuit_test_state();
        state.target = crate::provider::ProviderTarget::VscodeCopilot;
        state.client = reqwest::Client::builder().no_proxy().build().unwrap();
        let credential = crate::secrets::test_credentials::Credential::new("mock-key");
        state.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO providers (id,name,base_url,api_key,model,is_current,target_app,protocol_type)
                 VALUES ('context','context',?1,?2,'claude-sonnet-5.5',1,?3,'anthropic')",
                rusqlite::params![format!("http://{address}"), credential.reference(), state.target.as_str()],
            )?;
            Ok(())
        }).unwrap();
        let mut previous: Option<String> = None;
        for input in ["Remember BLUE742.", "What was the code?"] {
            let mut body = json!({"model":"claude-sonnet-5.5","input":input,"stream":streaming});
            if let Some(id) = &previous {
                body["previous_response_id"] = Value::String(id.clone());
            }
            let response = codex_proxy_handler(
                State(state.clone()),
                "/v1/responses".parse().unwrap(),
                Method::POST,
                HeaderMap::new(),
                Bytes::from(body.to_string()),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let result: Value = if streaming {
                std::str::from_utf8(&bytes)
                    .unwrap()
                    .split("\n\n")
                    .filter_map(parse_chat_sse_data)
                    .filter_map(|data| serde_json::from_str::<Value>(&data).ok())
                    .find(|event| event["type"] == "response.completed")
                    .unwrap()["response"]
                    .clone()
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            previous = Some(result["id"].as_str().unwrap().to_string());
        }
        server.abort();
        let captured = seen.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[1]["messages"].as_array().unwrap().len(), 3);
        assert_eq!(
            captured[1]["messages"][0]["content"][0]["text"],
            "Remember BLUE742."
        );
        assert_eq!(captured[1]["messages"][1]["content"][0]["text"], "已记住");
        assert_eq!(
            captured[1]["messages"][2]["content"][0]["text"],
            "What was the code?"
        );
    }
}

#[tokio::test]
async fn native_responses_retry_restores_history_only_after_previous_id_rejection() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let captured = Arc::clone(&seen);
    let mock = axum::Router::new().route("/v1/responses", axum::routing::post(
        move |axum::Json(body): axum::Json<Value>| {
            let captured = Arc::clone(&captured);
            async move {
                captured.lock().unwrap().push(body.clone());
                if body.get("previous_response_id").is_some() {
                    return (StatusCode::BAD_REQUEST, axum::Json(json!({
                        "error":{"message":"previous_response_id requires an OpenAI API-key account"}
                    }))).into_response();
                }
                axum::Json(json!({
                    "id":format!("resp_{}", uuid::Uuid::new_v4().simple()),
                    "object":"response", "status":"completed",
                    "output":[{"type":"message","role":"assistant",
                        "content":[{"type":"output_text","text":"已记住"}]}]
                })).into_response()
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let mut state = super::super::tests::circuit_test_state();
    state.target = crate::provider::ProviderTarget::VscodeCopilot;
    state.client = reqwest::Client::builder().no_proxy().build().unwrap();
    let credential = crate::secrets::test_credentials::Credential::new("mock-key");
    state.db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO providers (id,name,base_url,api_key,model,is_current,target_app,protocol_type)
             VALUES ('native-context','native-context',?1,?2,'gpt-test',1,?3,'openai_responses')",
            rusqlite::params![format!("http://{address}"), credential.reference(), state.target.as_str()],
        )?;
        Ok(())
    }).unwrap();
    let mut previous: Option<String> = None;
    for input in ["Remember BLUE742.", "Recall the code."] {
        let mut request = json!({"model":"gpt-test","input":input,"stream":false});
        if let Some(id) = &previous {
            request["previous_response_id"] = Value::String(id.clone());
        }
        let response = codex_proxy_handler(
            State(state.clone()),
            "/v1/responses".parse().unwrap(),
            Method::POST,
            HeaderMap::new(),
            Bytes::from(request.to_string()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        previous = Some(response["id"].as_str().unwrap().to_string());
    }
    server.abort();
    let captured = seen.lock().unwrap();
    assert_eq!(captured.len(), 3);
    assert!(captured[1].get("previous_response_id").is_some());
    assert_eq!(captured[1]["input"], "Recall the code.");
    assert!(captured[2].get("previous_response_id").is_none());
    assert_eq!(captured[2]["input"][0]["content"], "Remember BLUE742.");
    assert_eq!(captured[2]["input"][1]["content"][0]["text"], "已记住");
    assert_eq!(captured[2]["input"][2]["content"], "Recall the code.");
}

#[tokio::test]
#[ignore = "requires AISW_CONTEXT_TEST_KEY and a running local gateway; consumes upstream usage"]
async fn live_responses_context_through_installed_gateway() {
    use base64::Engine;
    let key = std::env::var("AISW_CONTEXT_TEST_KEY").expect("local gateway test key");
    let credential = crate::secrets::test_credentials::Credential::new(&key);
    let mut state = super::super::tests::circuit_test_state();
    state.target = crate::provider::ProviderTarget::VscodeCopilot;
    state.client = reqwest::Client::builder().no_proxy().build().unwrap();
    state.db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO providers (id,name,base_url,api_key,model,is_current,target_app,protocol_type)
             VALUES ('context-live','context-live','http://127.0.0.1:15828',?1,'claude-sonnet-5.5',1,?2,'anthropic')",
            rusqlite::params![credential.reference(), state.target.as_str()],
        )?;
        Ok(())
    }).unwrap();

    async fn call(state: &ProxyState, body: Value) -> Value {
        let mut headers = HeaderMap::new();
        headers.insert("x-ai-switcher-target", "vscode_copilot".parse().unwrap());
        headers.insert(
            "x-session-id",
            "context-fix-live-regression".parse().unwrap(),
        );
        let response = codex_proxy_handler(
            State(state.clone()),
            "/v1/responses".parse().unwrap(),
            Method::POST,
            headers,
            Bytes::from(body.to_string()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result = std::str::from_utf8(&bytes)
            .unwrap()
            .split("\n\n")
            .filter_map(parse_chat_sse_data)
            .filter_map(|data| serde_json::from_str::<Value>(&data).ok())
            .find(|event| event["type"] == "response.completed")
            .expect("completed response")["response"]
            .clone();
        println!(
            "model={} id={} text={}",
            result["model"],
            result["id"],
            text(&result)
        );
        result
    }
    fn text(response: &Value) -> String {
        response["output"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    }
    for models in [
        [
            "claude-sonnet-5.5",
            "claude-sonnet-5.5",
            "claude-sonnet-5.5",
        ],
        [
            "gemini-4-argon-low",
            "claude-sonnet-5.5",
            "gemini-4-argon-low",
        ],
    ] {
        let mut previous = Value::Null;
        for (round, model) in models.iter().enumerate() {
            let mut body = json!({"model":model,"stream":true,"max_output_tokens":256,
                "input": if round == 0 {
                    "这是模型切换测试，请记住暗号 BLUE742，只回复“已记住”，不要调用工具或修改文件。"
                } else { "刚才的暗号是什么？只回答暗号，不要调用工具。" }
            });
            if round > 0 {
                body["previous_response_id"] = previous;
            }
            let response = call(&state, body).await;
            if round > 0 {
                assert!(text(&response).contains("BLUE742"));
            }
            previous = response["id"].clone();
        }
    }

    let image =
        std::fs::read(std::env::var("AISW_CONTEXT_TEST_IMAGE").expect("red/blue PNG fixture"))
            .unwrap();
    let image = base64::engine::general_purpose::STANDARD.encode(image);
    let response = call(
        &state,
        json!({"model":"claude-sonnet-5.5","stream":true,"max_output_tokens":256,
            "input":[{"role":"user","content":[
                {"type":"input_text","text":"记住这张图片，只回复已记住，不要描述图片。"},
                {"type":"input_image","image_url":format!("data:image/png;base64,{image}")}
            ]}]
        }),
    )
    .await;
    let response = call(&state, json!({"model":"claude-sonnet-5.5","stream":true,"max_output_tokens":256,
        "previous_response_id":response["id"],"input":"刚才图片左半和右半分别是什么颜色？用中文简短回答。"
    })).await;
    assert!(text(&response).contains('红') && text(&response).contains('蓝'));

    let tools = json!([{"type":"function","name":"web_search","description":"Search documentation",
        "parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}]);
    let response = call(&state, json!({"model":"claude-sonnet-5.5","stream":true,"max_output_tokens":512,
        "tools":tools,"input":"请调用 web_search 搜索 Workspace Trust。拿到结果后，用一句中文总结，并在末尾附上暗号 BLUE742。"
    })).await;
    let tool = response["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "function_call")
        .expect("search tool call");
    let response = call(&state, json!({"model":"claude-sonnet-5.5","stream":true,"max_output_tokens":512,
        "tools":tools,"previous_response_id":response["id"],
        "input":[{"type":"function_call_output","call_id":tool["call_id"],
            "output":"Workspace Trust restricts code execution in untrusted folders. Source: VS Code documentation."}]
    })).await;
    assert!(text(&response).contains("BLUE742"));
}
