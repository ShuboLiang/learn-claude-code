//! 三层上下文压缩策略
//!
//! - 第一层（micro_compact）：将旧的工具结果替换为占位符
//! - 第二层（auto_compact）：token 超阈值时，保存完整对话并生成摘要
//! - 第三层（manual_compact）：AI 主动调用 compact 工具触发

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde_json::Value;

use super::history::Conversation;
use crate::AgentResult;
use crate::api::types::{ApiMessage, ProviderRequest};

/// auto_compact 触发的 token 估算阈值
pub const TOKEN_THRESHOLD: usize = 50_000;

/// micro_compact 保留的最近工具结果数量
const KEEP_RECENT: usize = 5;

/// 压缩后保留的摘要长度（字符数）
const SUMMARY_LEN: usize = 300;

/// transcript 保存目录名
const TRANSCRIPT_DIR_NAME: &str = ".transcripts";

/// 需要保留完整结果的工具名称
const PRESERVE_RESULT_TOOLS: &[&str] = &["read_file"];

/// 向后兼容的独立函数：估算 token 数（约 4 字符/token）
pub fn estimate_tokens(messages: &[ApiMessage]) -> usize {
    let json = serde_json::to_string(messages).unwrap_or_default();
    json.len() / 4
}

/// 按行截断文本，累计不超过 max_chars 字符
fn truncate_by_lines(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for line in text.lines() {
        let line_len = line.len() + 1;
        if result.len() + line_len > max_chars {
            break;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
    }
    if result.len() < text.len() {
        result.push_str("\n...");
    }
    result
}

/// 第一层压缩：将旧的工具结果替换为简短的占位符
pub fn micro_compact(conv: &mut Conversation) {
    let messages = conv.messages_mut();

    let mut tool_results: Vec<(usize, usize)> = Vec::new();
    for (msg_idx, msg) in messages.iter().enumerate() {
        if msg.role != "user" {
            continue;
        }
        if let Value::Array(ref parts) = msg.content {
            for (part_idx, part) in parts.iter().enumerate() {
                if part.get("type").and_then(Value::as_str) == Some("tool_result") {
                    tool_results.push((msg_idx, part_idx));
                }
            }
        }
    }

    if tool_results.len() <= KEEP_RECENT {
        return;
    }

    let mut tool_name_map: HashMap<String, String> = HashMap::new();
    for msg in messages.iter() {
        if msg.role != "assistant" {
            continue;
        }
        if let Value::Array(ref blocks) = msg.content {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("tool_use")
                    && let (Some(id), Some(name)) = (
                        block.get("id").and_then(Value::as_str),
                        block.get("name").and_then(Value::as_str),
                    )
                {
                    tool_name_map.insert(id.to_owned(), name.to_owned());
                }
            }
        }
    }

    let to_clear = &tool_results[..tool_results.len() - KEEP_RECENT];
    for &(msg_idx, part_idx) in to_clear {
        if let Value::Array(ref mut parts) = messages[msg_idx].content
            && let Some(part) = parts.get_mut(part_idx)
        {
            if let Some(content) = part.get("content").and_then(Value::as_str)
                && content.len() <= 100
            {
                continue;
            }

            let tool_id = part
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let tool_name = tool_name_map
                .get(tool_id)
                .map(|s| s.as_str())
                .unwrap_or("unknown");

            if PRESERVE_RESULT_TOOLS.contains(&tool_name) {
                continue;
            }

            let original = part.get("content").and_then(Value::as_str).unwrap_or("");
            let summary = truncate_by_lines(original, SUMMARY_LEN);
            let original_chars = original.chars().count();
            part["content"] = Value::String(format!(
                "[已压缩: {tool_name}, 原文 {original_chars} 字符]\n{summary}"
            ));
        }
    }
}

/// 第二层/第三层压缩：保存完整对话到磁盘，调用 LLM 生成摘要
pub async fn auto_compact(
    client: &crate::api::LlmProvider,
    model: &str,
    workspace_root: &Path,
    conv: &Conversation,
) -> AgentResult<Vec<ApiMessage>> {
    let transcript_dir = workspace_root.join(TRANSCRIPT_DIR_NAME);
    std::fs::create_dir_all(&transcript_dir)
        .with_context(|| format!("创建 {} 目录失败", TRANSCRIPT_DIR_NAME))?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let transcript_path = transcript_dir.join(format!("transcript_{timestamp}.jsonl"));

    {
        let mut file = std::fs::File::create(&transcript_path)
            .with_context(|| format!("创建 transcript 文件失败: {}", transcript_path.display()))?;
        for msg in conv.messages() {
            let line = serde_json::to_string(msg)?;
            writeln!(file, "{line}")?;
        }
    }
    println!("[transcript 已保存: {}]", transcript_path.display());

    let conversation_text = serde_json::to_string(conv.messages())?;
    let truncated: &str = if conversation_text.len() > 80_000 {
        &conversation_text[conversation_text.len() - 80_000..]
    } else {
        &conversation_text
    };

    let summary_messages = vec![ApiMessage::user_text(format!(
        "请总结这段对话，以便后续继续工作。包括：\n\
         1) 完成了什么\n\
         2) 当前状态\n\
         3) 做了哪些关键决策\n\n\
         请简洁但保留关键细节。\n\n{truncated}"
    ))];

    let request = ProviderRequest {
        model,
        system: "你是一个对话摘要助手。请简洁地总结对话内容。",
        messages: &summary_messages,
        tools: &[],
        max_tokens: 2000,
    };

    let response = client.create_message(&request, None, None).await?;
    let summary = response.final_text();

    Ok(vec![ApiMessage::user_text(format!(
        "[对话已压缩。完整记录: {}]\n\n{summary}",
        transcript_path.display()
    ))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn make_assistant_with_tool_use(id: &str, name: &str) -> ApiMessage {
        ApiMessage {
            role: "assistant".to_owned(),
            content: json!([
                { "type": "tool_use", "id": id, "name": name, "input": {} }
            ]),
        }
    }

    fn make_user_with_tool_result(tool_use_id: &str, content: &str) -> ApiMessage {
        ApiMessage {
            role: "user".to_owned(),
            content: json!([
                { "type": "tool_result", "tool_use_id": tool_use_id, "content": content }
            ]),
        }
    }

    #[test]
    fn micro_compact_preserves_recent_results() {
        let mut conv = Conversation::new();
        conv.push(make_assistant_with_tool_use("id1", "bash"));
        conv.push(make_user_with_tool_result("id1", &"x".repeat(200)));
        conv.push(make_assistant_with_tool_use("id2", "bash"));
        conv.push(make_user_with_tool_result("id2", &"y".repeat(200)));

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            assert!(
                parts[0]
                    .get("content")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .starts_with("x")
            );
        }
        if let Value::Array(ref parts) = msgs[3].content {
            assert!(
                parts[0]
                    .get("content")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .starts_with("y")
            );
        }
    }

    #[test]
    fn micro_compact_replaces_old_results() {
        let mut conv = Conversation::new();
        for (id, ch) in [
            ("id1", "a"),
            ("id2", "b"),
            ("id3", "c"),
            ("id4", "d"),
            ("id5", "e"),
            ("id6", "f"),
            ("id7", "g"),
        ] {
            conv.push(make_assistant_with_tool_use(id, "bash"));
            conv.push(make_user_with_tool_result(id, &ch.repeat(200)));
        }

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert!(content.contains("[已压缩: bash"));
        }
        if let Value::Array(ref parts) = msgs[5].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert!(content.starts_with("c"));
        }
    }

    #[test]
    fn micro_compact_preserves_read_file_results() {
        let mut conv = Conversation::new();
        conv.push(make_assistant_with_tool_use("id1", "read_file"));
        conv.push(make_user_with_tool_result(
            "id1",
            &"file content here".repeat(20),
        ));
        for (id, ch) in [("id2", "o"), ("id3", "o"), ("id4", "o"), ("id5", "o")] {
            conv.push(make_assistant_with_tool_use(id, "bash"));
            conv.push(make_user_with_tool_result(id, &ch.repeat(30)));
        }

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert!(content.starts_with("file content here"));
        }
    }

    #[test]
    fn micro_compact_skips_short_content() {
        let mut conv = Conversation::new();
        conv.push(make_assistant_with_tool_use("id1", "bash"));
        conv.push(make_user_with_tool_result("id1", "short"));
        for (id, ch) in [("id2", "x"), ("id3", "x"), ("id4", "x"), ("id5", "x")] {
            conv.push(make_assistant_with_tool_use(id, "bash"));
            conv.push(make_user_with_tool_result(id, &ch.repeat(200)));
        }

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert_eq!(content, "short");
        }
    }

    #[test]
    fn conversation_estimate_tokens() {
        let mut conv = Conversation::new();
        conv.push_user_text("hello world");
        let tokens = conv.estimate_tokens();
        assert!(tokens > 0 && tokens < 100);
    }

    #[test]
    fn standalone_estimate_tokens() {
        let messages = vec![ApiMessage::user_text("hello world")];
        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0 && tokens < 100);
    }

    #[test]
    fn conversation_clear() {
        let mut conv = Conversation::new();
        conv.push_user_text("msg1");
        conv.push_user_text("msg2");
        conv.push_user_text("msg3");
        let cleared = conv.clear();
        assert_eq!(cleared, 3);
        assert!(conv.is_empty());
    }
}
