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
    assert!(json.get("version").is_some());
}

#[tokio::test]
async fn sync_message_creates_task() {
    let (client, base_url) = start_test_server().await;

    let payload = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        }
    });

    let res = client
        .post(format!("{}/message:send", base_url))
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let json: serde_json::Value = res.json().await.unwrap();
    assert!(json.get("id").is_some()); // server-generated task id
    assert!(json.get("status").is_some());
    assert!(json.get("contextId").is_some());
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

#[tokio::test]
async fn cancel_nonexistent_task_returns_404() {
    let (client, base_url) = start_test_server().await;

    let res = client
        .post(format!("{}/tasks/nonexistent/cancel", base_url))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_completed_task_returns_not_cancelable() {
    let (client, base_url) = start_test_server().await;

    // Blocking send completes immediately in test env (fast agent mock).
    let create = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        }
    });
    let res = client
        .post(format!("{}/message:send", base_url))
        .json(&create)
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = res.json().await.unwrap();
    let task_id = task["id"].as_str().unwrap();

    // Task is already Completed, so cancel should be rejected.
    let cancel_res = client
        .post(format!("{}/tasks/{}/cancel", base_url, task_id))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(cancel_res.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = cancel_res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "TaskNotCancelableError");
}

#[tokio::test]
async fn list_tasks_returns_created_task() {
    let (client, base_url) = start_test_server().await;

    let payload = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        }
    });
    let res = client
        .post(format!("{}/message:send", base_url))
        .json(&payload)
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = res.json().await.unwrap();

    let list_res = client
        .get(format!("{}/tasks", base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(list_res.status(), StatusCode::OK);
    let body: serde_json::Value = list_res.json().await.unwrap();
    assert!(body["tasks"].as_array().unwrap().len() >= 1);
    assert!(body.get("pageSize").is_some());
    assert!(body.get("totalSize").is_some());
}

#[tokio::test]
async fn subscribe_task_returns_snapshot() {
    let (client, base_url) = start_test_server().await;

    // Use returnImmediately so the task stays in Working state and can be subscribed.
    let payload = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        },
        "configuration": {
            "returnImmediately": true
        }
    });
    let res = client
        .post(format!("{}/message:send", base_url))
        .json(&payload)
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = res.json().await.unwrap();
    let task_id = task["id"].as_str().unwrap();

    let sub_res = client
        .post(format!("{}/tasks/{}/subscribe", base_url, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(sub_res.status(), StatusCode::OK);
    assert_eq!(
        sub_res.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
}

#[tokio::test]
async fn extended_agent_card_returns_error_when_disabled() {
    let (client, base_url) = start_test_server().await;

    let res = client
        .get(format!("{}/extendedAgentCard", base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "UnsupportedOperationError");
}

#[tokio::test]
async fn push_notification_returns_not_supported() {
    let (client, base_url) = start_test_server().await;

    // Create a task first, then verify push notifications are not supported.
    let create = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        }
    });
    let create_res = client
        .post(format!("{}/message:send", base_url))
        .json(&create)
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = create_res.json().await.unwrap();
    let task_id = task["id"].as_str().unwrap();

    let res = client
        .post(format!("{}/tasks/{}/pushNotificationConfigs", base_url, task_id))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "PushNotificationNotSupportedError");
}
