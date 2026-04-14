//! 通用文本工具函数

/// 将文本截断到指定字符数，超出部分直接丢弃
///
/// 所有工具输出的截断统一使用此函数。
pub fn truncate_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// 截取文本的前 N 个字符，超出部分用 "..." 省略
///
/// 用于日志和终端显示，避免打印过长内容。
pub fn preview_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let head: String = text.chars().take(max_chars).collect();
    format!("{head}...")
}
