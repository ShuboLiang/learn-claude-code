//! 通用文本工具函数

/// 将文本截断到指定字符数，超出部分附带省略提示
///
/// 所有工具输出的截断统一使用此函数。
pub fn truncate_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        text.to_owned()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{truncated}\n\n... [输出已截断，共 {char_count} 字符，仅显示前 {max_chars} 字符]")
    }
}

/// 截取文本的前 N 个字符，超出部分用省略号提示剩余数量
///
/// 用于 SSE 事件和终端显示，避免传输过长内容。
pub fn preview_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        text.to_owned()
    } else {
        let head: String = text.chars().take(max_chars).collect();
        let remaining = char_count - max_chars;
        format!("{head}\n... [还有 {remaining} 字符未显示]")
    }
}
