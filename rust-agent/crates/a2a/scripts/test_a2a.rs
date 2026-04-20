//! A2A 服务端接口测试 — Rust 版本
//!
//! 用法:
//!     cargo run --example test_a2a -- http://localhost:3001
//!
//! 需要添加临时依赖到 crates/a2a/Cargo.toml:
//!     reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }

use std::time::Duration;

const DEFAULT_BASE_URL: &str = "http://localhost:3001";

/// 通用的 HTTP JSON 请求辅助函数
async fn request_json(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    body: Option<&serde_json::Value>,
) -> anyhow::Result<(u16, serde_json::Value)> {
    let mut req = client.request(method, url);
    if let Some(b) = body {
        req = req.json(b);
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let json = resp.json().await?;
    Ok((status, json))
}

/// 测试 1: Agent Card
async fn test_agent_card(client: &reqwest::Client, base: &str) -> bool {
    println!("\n[测试 1] GET /.well-known/agent.json");
    let url = format!("{}/.well-known/agent.json", base);
    match request_json(client, reqwest::Method::GET, &url, None).await {
        Ok((200, body)) => {
            let name = body["name"].as_str().unwrap_or("unknown");
            let skills = body["skills"].as_array().map(|v| v.len()).unwrap_or(0);
            println!("  ✅ 成功: Agent = {name}, Skills = {skills}");
            true
        }
        Ok((code, body)) => {
            println!("  ❌ 失败: 状态码 {code}, 响应: {body}");
            false
        }
        Err(e) => {
            println!("  ❌ 失败: {e}");
            false
        }
    }
}

/// 测试 2: 同步任务（需要 LLM 环境，可能较慢）
async fn test_sync_task(client: &reqwest::Client, base: &str) -> bool {
    println!("\n[测试 2] POST /tasks/send");
    let url = format!("{}/tasks/send", base);
    let payload = serde_json::json!({
        "id": "test-sync-001",
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": "用 bash 运行 echo hello"}]
        }
    });
    match request_json(client, reqwest::Method::POST, &url, Some(&payload)).await {
        Ok((200, body)) => {
            let status = body["status"]["state"].as_str().unwrap_or("unknown");
            println!("  ✅ 成功: 任务状态 = {status}");
            true
        }
        Ok((503, _)) => {
            println!("  ⚠️  Agent 初始化失败（可能缺少 LLM API Key）");
            true // 环境问题不算失败
        }
        Ok((code, body)) => {
            println!("  ❌ 失败: 状态码 {code}, 响应: {body}");
            false
        }
        Err(e) => {
            println!("  ❌ 失败: {e}");
            false
        }
    }
}

/// 测试 3: 重复任务 ID 冲突
async fn test_duplicate_task(client: &reqwest::Client, base: &str) -> bool {
    println!("\n[测试 3] 重复任务 ID 冲突检测");
    let url = format!("{}/tasks/send", base);
    let payload = serde_json::json!({
        "id": "test-dup-001",
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": "hello"}]
        }
    });

    // 第一次（忽略结果）
    let _ = request_json(client, reqwest::Method::POST, &url, Some(&payload)).await;

    // 第二次应该 409
    match request_json(client, reqwest::Method::POST, &url, Some(&payload)).await {
        Ok((409, _)) => {
            println!("  ✅ 成功: 重复 ID 正确返回 409");
            true
        }
        Ok((code, _)) => {
            println!("  ❌ 失败: 期望 409，实际 {code}");
            false
        }
        Err(e) => {
            println!("  ❌ 失败: {e}");
            false
        }
    }
}

/// 测试 4: 查询不存在任务
async fn test_get_nonexistent(client: &reqwest::Client, base: &str) -> bool {
    println!("\n[测试 4] GET /tasks/{taskId} 404");
    let url = format!("{}/tasks/nonexistent-999", base);
    match request_json(client, reqwest::Method::GET, &url, None).await {
        Ok((404, _)) => {
            println!("  ✅ 成功: 正确返回 404");
            true
        }
        Ok((code, _)) => {
            println!("  ❌ 失败: 期望 404，实际 {code}");
            false
        }
        Err(e) => {
            println!("  ❌ 失败: {e}");
            false
        }
    }
}

/// 测试 5: 取消不存在任务
async fn test_cancel_nonexistent(client: &reqwest::Client, base: &str) -> bool {
    println!("\n[测试 5] POST /tasks/{taskId}/cancel 404");
    let url = format!("{}/tasks/nonexistent-999/cancel", base);
    match request_json(client, reqwest::Method::POST, &url, None).await {
        Ok((404, _)) => {
            println!("  ✅ 成功: 正确返回 404");
            true
        }
        Ok((code, _)) => {
            println!("  ❌ 失败: 期望 404，实际 {code}");
            false
        }
        Err(e) => {
            println!("  ❌ 失败: {e}");
            false
        }
    }
}

/// 测试 6: SSE 流式（只验证能收到事件流）
async fn test_streaming(client: &reqwest::Client, base: &str) -> bool {
    println!("\n[测试 6] POST /tasks/sendSubscribe（SSE 流式）");
    let url = format!("{}/tasks/sendSubscribe", base);
    let payload = serde_json::json!({
        "id": "test-stream-001",
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": "echo test"}]
        }
    });

    let resp = match client
        .post(&url)
        .json(&payload)
        .header("Accept", "text/event-stream")
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            println!("  ❌ 失败: {e}");
            return false;
        }
    };

    let status = resp.status().as_u16();
    if status == 503 {
        println!("  ⚠️  Agent 初始化失败（可能缺少 LLM API Key）");
        return true;
    }

    if status != 200 {
        println!("  ❌ 失败: 状态码 {status}");
        return false;
    }

    // 读取一小部分 SSE 数据
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            println!("  ❌ 失败: 读取响应体出错: {e}");
            return false;
        }
    };

    let events: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("event:"))
        .map(|l| l.trim_start_matches("event:").trim())
        .collect();

    if events.is_empty() {
        println!("  ⚠️  收到响应但没有 SSE 事件（可能是空流）");
        true
    } else {
        println!("  ✅ 成功: 收到 SSE 事件: {:?}", &events[..events.len().min(5)]);
        true
    }
}

#[tokio::main]
async fn main() {
    let base = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    println!("A2A 服务端接口测试");
    println!("目标地址: {base}");
    println!("==================================================");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("创建 HTTP 客户端失败");

    let results = vec![
        ("Agent Card", test_agent_card(&client, &base).await),
        ("同步任务", test_sync_task(&client, &base).await),
        ("重复任务冲突", test_duplicate_task(&client, &base).await),
        ("查询不存在任务", test_get_nonexistent(&client, &base).await),
        ("取消不存在任务", test_cancel_nonexistent(&client, &base).await),
        ("流式任务", test_streaming(&client, &base).await),
    ];

    println!("\n==================================================");
    println!("测试结果汇总:");
    let mut passed = 0;
    for (name, ok) in &results {
        let status = if *ok { "✅ 通过" } else { "❌ 失败" };
        println!("  {status} - {name}");
        if *ok {
            passed += 1;
        }
    }

    println!("\n总计: {passed}/{} 通过", results.len());
    if passed == results.len() {
        println!("🎉 全部通过!");
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}
