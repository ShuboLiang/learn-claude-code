use anyhow::Context;

use crate::AgentResult;
use crate::infra::utils::truncate_text;
use crate::infra::workspace::resolve_workspace_path;

/// 生成 unified diff 格式的变更摘要（类似 Python difflib.unified_diff）
fn generate_unified_diff(old: &str, new: &str, filename: &str) -> String {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();

    // 找到第一个不同行的位置
    let mut start = 0usize;
    while start < old_lines.len()
        && start < new_lines.len()
        && old_lines[start] == new_lines[start]
    {
        start += 1;
    }

    // 找到最后一个不同行的位置
    let mut old_end = old_lines.len();
    let mut new_end = new_lines.len();
    while old_end > start
        && new_end > start
        && old_lines[old_end - 1] == new_lines[new_end - 1]
    {
        old_end -= 1;
        new_end -= 1;
    }

    // 无变化时返回空
    if start >= old_lines.len() && start >= new_lines.len() {
        return String::new();
    }

    let context = 3usize;
    let ctx_start = start.saturating_sub(context);
    let old_ctx_end = (old_end + context).min(old_lines.len());
    let new_ctx_end = (new_end + context).min(new_lines.len());

    let mut diff = format!("--- {filename}\n+++ {filename}\n");
    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        ctx_start + 1,
        old_ctx_end - ctx_start,
        ctx_start + 1,
        new_ctx_end - ctx_start,
    ));

    // 合并遍历旧行和新行，输出统一的 diff hunk
    let max_ctx = old_ctx_end.max(new_ctx_end);
    let mut old_idx = ctx_start;
    let mut new_idx = ctx_start;

    while old_idx < max_ctx || new_idx < max_ctx {
        let in_old = old_idx < old_lines.len();
        let in_new = new_idx < new_lines.len();
        let old_line = if in_old { old_lines[old_idx] } else { "" };
        let new_line = if in_new { new_lines[new_idx] } else { "" };

        let old_changed = old_idx >= start && old_idx < old_end;
        let new_changed = new_idx >= start && new_idx < new_end;

        if old_changed && new_changed {
            // 两行都被修改，先输出删除再输出新增
            if in_old {
                diff.push_str(&format!("-{}", old_line));
            }
            if in_new {
                diff.push_str(&format!("+{}", new_line));
            }
            old_idx += 1;
            new_idx += 1;
        } else if old_changed {
            // 只有旧行被删除
            if in_old {
                diff.push_str(&format!("-{}", old_line));
            }
            old_idx += 1;
        } else if new_changed {
            // 只有新行被新增
            if in_new {
                diff.push_str(&format!("+{}", new_line));
            }
            new_idx += 1;
        } else {
            // 上下文行（未变化）
            if in_old {
                diff.push_str(&format!(" {}", old_line));
            }
            old_idx += 1;
            new_idx += 1;
        }
    }

    if !diff.ends_with('\n') {
        diff.push('\n');
    }

    diff
}

impl super::AgentToolbox {
    /// 读取指定文件的内容
    ///
    /// 路径会通过 `resolve_workspace_path` 安全校验。
    /// 可选 `limit` 限制读取行数，超出部分会截断并显示剩余行数。
    pub(crate) fn read_file(&self, path: &str, limit: Option<usize>) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, path)?;
        let content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read {}", resolved.display()))?;
        let mut lines = content.lines().map(str::to_owned).collect::<Vec<_>>();
        if let Some(limit) = limit
            && limit < lines.len()
        {
            let remaining = lines.len() - limit;
            lines.truncate(limit);
            lines.push(format!("... ({remaining} more lines)"));
        }
        Ok(truncate_text(&lines.join("\n"), 50_000))
    }

    /// 将内容写入指定文件
    ///
    /// 如果目标文件的父目录不存在会自动创建。
    pub(crate) fn write_file(&self, path: &str, content: &str) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, path)?;
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(&resolved, content)
            .with_context(|| format!("Failed to write {}", resolved.display()))?;
        Ok(format!("已写入 {} 字节", content.len()))
    }

    /// 在文件中精确替换文本，返回 unified diff 格式的变更摘要
    ///
    /// 使用二进制读写保留原始行尾（CRLF/LF），与 Python file_edit_tool.py 行为一致。
    /// `replace_all` 为 true 时替换所有匹配项，否则只替换首次出现。
    /// 如果 old_string 出现多次且 replace_all 为 false，返回错误要求消歧。
    pub(crate) fn edit_file(
        &self,
        file_path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, file_path)?;

        // 校验文件存在
        if !resolved.is_file() {
            return Ok(format!("错误：文件不存在：{}", resolved.display()));
        }

        // 校验 old_string 与 new_string 不同
        if old_string == new_string {
            return Ok("错误：old_string 与 new_string 必须不同".to_string());
        }

        // 二进制读取保留原始行尾（避免 read_to_string 在 Windows 上做换行转换）
        let raw_bytes = std::fs::read(&resolved)
            .with_context(|| format!("无法读取文件：{}", resolved.display()))?;
        let content = String::from_utf8(raw_bytes)
            .with_context(|| format!("文件不是有效的 UTF-8：{}", resolved.display()))?;

        // 检查 old_string 是否存在于文件中
        let count = content.matches(old_string).count();
        if count == 0 {
            return Ok(format!("错误：在 {} 中未找到 old_string", file_path));
        }

        // 重复检测：多次出现但未指定 replace_all 时报错要求消歧
        if count > 1 && !replace_all {
            return Ok(format!(
                "错误：old_string 在 {} 中出现了 {} 次。请设置 replace_all: true 替换全部，或提供更多上下文使匹配唯一。",
                file_path, count
            ));
        }

        // 执行替换
        let updated = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // 二进制写回保留原始行尾
        std::fs::write(&resolved, updated.as_bytes())
            .with_context(|| format!("无法写入文件：{}", resolved.display()))?;

        // 生成 unified diff
        let diff = generate_unified_diff(&content, &updated, file_path);
        let result = if diff.is_empty() {
            "文件已更新（无可视差异）".to_string()
        } else {
            diff
        };

        Ok(truncate_text(&result, 50_000))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::skills::SkillLoader;
    use super::super::AgentToolbox;

    // 每个测试创建唯一的临时子目录避免文件冲突
    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 创建测试用的工具箱实例，工作区根目录为临时目录（每次调用生成唯一子目录）
    fn test_toolbox() -> (AgentToolbox, std::path::PathBuf) {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!("rust-agent-test-{id}"));
        let _ = std::fs::create_dir_all(&tmp);
        let skills = SkillLoader::default();
        let toolbox = AgentToolbox::new(
            tmp.clone(),
            Arc::new(RwLock::new(skills)),
            vec![],
        );
        (toolbox, tmp)
    }

    /// 在工具箱的工作区中创建测试文件
    fn write_test_file(toolbox: &AgentToolbox, name: &str, content: &str) {
        let path = toolbox.workspace_root.join(name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, content).expect("创建测试文件失败");
    }

    /// 在工具箱工作区中读取测试文件
    fn read_test_file(toolbox: &AgentToolbox, name: &str) -> String {
        let path = toolbox.workspace_root.join(name);
        std::fs::read_to_string(&path).expect("读取测试文件失败")
    }

    // ── 基本替换测试 ──

    #[test]
    fn edit_file_替换单个匹配文本并返回diff() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "hello world\n");
        let result = tb.edit_file("test.txt", "hello", "hi", false).unwrap();
        assert!(result.starts_with("--- test.txt"), "应返回diff，实际: {result}");
        assert!(result.contains("-hello"), "diff应包含删除行");
        assert!(result.contains("+hi"), "diff应包含新增行");
        assert_eq!(read_test_file(&tb, "test.txt"), "hi world\n");
    }

    #[test]
    fn edit_file_替换所有匹配项() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "a a a\n");
        let result = tb.edit_file("test.txt", "a", "b", true).unwrap();
        assert!(result.contains("--- test.txt"), "应返回diff");
        assert_eq!(read_test_file(&tb, "test.txt"), "b b b\n");
    }

    #[test]
    fn edit_file_不指定replace_all时只替换首次匹配() {
        let (tb, _tmp) = test_toolbox();
        // 使用唯一的 old_string 避免触发重复检测
        write_test_file(&tb, "test.txt", "foo bar baz\n");
        tb.edit_file("test.txt", "foo", "FOO", false).unwrap();
        assert_eq!(read_test_file(&tb, "test.txt"), "FOO bar baz\n", "只替换第一个foo");
    }

    // ── 错误处理测试 ──

    #[test]
    fn edit_file_文件不存在时返回错误() {
        let (tb, _tmp) = test_toolbox();
        let result = tb.edit_file("nonexistent.txt", "x", "y", false).unwrap();
        assert!(result.contains("错误"), "应返回错误信息");
        assert!(result.contains("文件不存在"), "应提示文件不存在");
    }

    #[test]
    fn edit_file_新老字符串相同时返回错误() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "content\n");
        let result = tb.edit_file("test.txt", "content", "content", false).unwrap();
        assert!(result.contains("old_string 与 new_string 必须不同"));
    }

    #[test]
    fn edit_file_未找到old_string时返回错误() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "hello\n");
        let result = tb.edit_file("test.txt", "world", "earth", false).unwrap();
        assert!(result.contains("未找到 old_string"));
    }

    #[test]
    fn edit_file_多次出现且未指定replace_all时返回错误() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "dup dup\n");
        let result = tb.edit_file("test.txt", "dup", "unique", false).unwrap();
        assert!(result.contains("出现了 2 次"), "应提示重复次数");
        assert!(result.contains("replace_all"), "应提示使用 replace_all");
    }

    // ── CRLF 行尾保留测试 ──

    #[test]
    fn edit_file_保留_crlf行尾() {
        let (tb, _tmp) = test_toolbox();
        let content_with_crlf = "line1\r\nline2\r\n";
        write_test_file(&tb, "crlf.txt", content_with_crlf);
        tb.edit_file("crlf.txt", "line1", "LINE1", false).unwrap();
        let path = tb.workspace_root.join("crlf.txt");
        let actual_bytes = std::fs::read(&path).unwrap();
        let actual = String::from_utf8(actual_bytes).unwrap();
        assert_eq!(actual, "LINE1\r\nline2\r\n", "CRLF行尾应被保留");
    }

    #[test]
    fn edit_file_保留_lf行尾() {
        let (tb, _tmp) = test_toolbox();
        let content_with_lf = "line1\nline2\n";
        write_test_file(&tb, "lf.txt", content_with_lf);
        tb.edit_file("lf.txt", "line1", "LINE1", false).unwrap();
        assert_eq!(read_test_file(&tb, "lf.txt"), "LINE1\nline2\n", "LF行尾应被保留");
    }

    // ── diff 格式测试 ──

    #[test]
    fn edit_file_diff包含unified_diff格式头() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "AAA\nBBB\nCCC\n");
        let result = tb.edit_file("test.txt", "BBB", "DDD", false).unwrap();
        assert!(result.contains("--- test.txt"), "应有源文件标记：\n{result}");
        assert!(result.contains("+++ test.txt"), "应有目标文件标记：\n{result}");
        assert!(result.contains("@@"), "应有hunk头：\n{result}");
    }

    #[test]
    fn edit_file_diff包含上下文行() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "A\nB\nC\nD\nE\nF\nG\n");
        let result = tb.edit_file("test.txt", "D", "DDD", false).unwrap();
        // 应包含上下文行（以空格开头）
        assert!(result.contains(" C\n"), "应包含上下文行C：\n{result}");
        assert!(result.contains(" E\n"), "应包含上下文行E：\n{result}");
    }
}
