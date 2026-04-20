use reqwest::StatusCode;
use std::time::Duration;

async fn start_test_server() -> (reqwest::Client, String) {
    let app = rust_agent_a2a::app("http://localhost:3001")
        .await
        .expect("Failed to build app");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);
    (client, base_url)
}

#[tokio::test]
async fn agent_card_returns_valid_json() {
    let (client, base_url) = start_test_server().await;

    let res = client
        .get(format!("{}/.well-known/agent.json", base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json: serde_json::Value = res.json().await.unwrap();
    assert!(json.get("name").is_some());
    assert!(json.get("skills").is_some());
}

#[tokio::test]
async fn sync_task_rejects_duplicate_id() {
    let (client, base_url) = start_test_server().await;

    let payload = serde_json::json!({
        "id": "dup-task",
        "message": {
            "role": "user",
            "parts": [{ "type": "text", "text": "hello" }]
        }
    });

    let _res1 = client
        .post(format!("{}/tasks/send", base_url))
        .json(&payload)
        .send()
        .await
        .unwrap();

    let res2 = client
        .post(format!("{}/tasks/send", base_url))
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(res2.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn get_nonexistent_task_returns_404() {
    let (client, base_url) = start_test_server().await;

    let res = client
        .get(format!("{}/tasks/nonexistent", base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
