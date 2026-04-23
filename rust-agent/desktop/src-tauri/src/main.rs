#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::Mutex;

struct ServerState {
    port: u16,
    #[allow(dead_code)]
    child: tokio::process::Child,
}

#[tauri::command]
async fn start_server(state: tauri::State<'_, Mutex<Option<ServerState>>>) -> Result<u16, String> {
    let mut lock = state.lock().await;
    if let Some(ref s) = *lock {
        return Ok(s.port);
    }

    let port = portpicker::pick_unused_port().ok_or("无法找到可用端口")?;

    #[cfg(target_os = "windows")]
    let binary = "rust-agent-server.exe";
    #[cfg(not(target_os = "windows"))]
    let binary = "rust-agent-server";

    let project_root = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .parent()
        .and_then(|p| p.parent())
        .ok_or("无法解析项目根目录")?
        .join("target/debug")
        .join(binary);

    let child = Command::new(&project_root)
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 server 失败: {e}"))?;

    // 等待 server 就绪（最多 10 秒）
    for _ in 0..100 {
        if reqwest::get(format!("http://127.0.0.1:{port}/"))
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            *lock = Some(ServerState { port, child });
            return Ok(port);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Err("server 启动超时".to_string())
}

#[tauri::command]
async fn stop_server(state: tauri::State<'_, Mutex<Option<ServerState>>>) -> Result<(), String> {
    let mut lock = state.lock().await;
    if let Some(mut s) = lock.take() {
        let _ = s.child.kill().await;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(Option::<ServerState>::None))
        .invoke_handler(tauri::generate_handler![start_server, stop_server])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn main() {
    run();
}
