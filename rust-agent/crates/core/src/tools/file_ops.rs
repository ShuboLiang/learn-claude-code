use std::borrow::Cow;

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

/// 计算两个字符串的 Levenshtein 编辑距离（用于模糊匹配诊断）
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev = vec![0; n + 1];
    let mut curr = vec![0; n + 1];

    for j in 0..=n {
        prev[j] = j;
    }

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (curr[j - 1] + 1)
                .min(prev[j] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

impl super::AgentToolbox {
    /// 读取指定文件的内容
    ///
    /// 路径会通过 `resolve_workspace_path` 安全校验。
    /// 可选 `offset` 跳过的行数（从 1 开始计数），可选 `limit` 限制读取行数，
    /// 超出部分会截断并显示剩余行数。
    pub(crate) fn read_file(
        &self,
        path: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, path)?;
        let content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read {}", resolved.display()))?;
        let all_lines = content.lines().map(str::to_owned).collect::<Vec<_>>();

        let skip = offset.unwrap_or(0).saturating_sub(1); // offset 从 1 开始
        let total = all_lines.len();
        let mut lines: Vec<String> = all_lines.into_iter().skip(skip).collect();

        if lines.is_empty() && skip >= total && total > 0 {
            return Ok(format!("... (文件共 {total} 行，offset 已超出范围)"));
        }

        let remaining_after_skip = total.saturating_sub(skip);
        if let Some(limit) = limit
            && limit < lines.len()
        {
            let remaining = remaining_after_skip - limit;
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
    ///
    /// **容错机制**：如果精确匹配失败，会自动尝试去除 old_string 首尾多余的
    /// 换行符或空白字符后再匹配（LLM 复制代码时常犯此错误）。
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

        // 尝试精确匹配，失败时自动容错（处理首尾多余空白/换行）
        let (effective_old, count, was_fuzzy) = match Self::resolve_old_string(old_string, &content) {
            Ok(v) => v,
            Err(diagnostic) => {
                return Ok(format!(
                    "错误：在 {} 中未找到 old_string。{}",
                    file_path, diagnostic
                ));
            }
        };

        // 重复检测：多次出现但未指定 replace_all 时报错要求消歧
        if count > 1 && !replace_all {
            return Ok(format!(
                "错误：old_string 在 {} 中出现了 {} 次。请设置 replace_all: true 替换全部，或提供更多上下文使匹配唯一。",
                file_path, count
            ));
        }

        // 如果 old_string 与 effective_old 的差异仅在于换行符（\r\n vs \n），
        // 则同步转换 new_string，避免把 LF 插入到 CRLF 文件中破坏行尾一致性
        let new_string_normalized = {
            let old_norm = old_string.replace("\r\n", "\n");
            let eff_norm = effective_old.replace("\r\n", "\n");
            if old_norm == eff_norm && old_string != effective_old.as_ref() {
                if !old_string.contains("\r\n") && !new_string.contains("\r\n") {
                    // old_string 和 new_string 都是 LF，但文件是 CRLF
                    Cow::Owned(new_string.replace('\n', "\r\n"))
                } else if old_string.contains("\r\n") && new_string.contains("\r\n") {
                    // old_string 和 new_string 都是 CRLF，但文件是 LF
                    Cow::Owned(new_string.replace("\r\n", "\n"))
                } else {
                    Cow::Borrowed(new_string)
                }
            } else {
                Cow::Borrowed(new_string)
            }
        };

        // 执行替换
        let updated = if replace_all {
            content.replace(effective_old.as_ref(), new_string_normalized.as_ref())
        } else {
            content.replacen(effective_old.as_ref(), new_string_normalized.as_ref(), 1)
        };

        // 二进制写回保留原始行尾
        std::fs::write(&resolved, updated.as_bytes())
            .with_context(|| format!("无法写入文件：{}", resolved.display()))?;

        // 生成 unified diff
        let diff = generate_unified_diff(&content, &updated, file_path);
        let mut result = if diff.is_empty() {
            "文件已更新（无可视差异）".to_string()
        } else {
            diff
        };

        if was_fuzzy {
            result.push_str("\n[提示] old_string 与文件中的文本不完全一致，已自动忽略首尾空白/换行差异完成替换。建议下次使用 read_file 获取的精确文本。");
        }

        Ok(truncate_text(&result, 50_000))
    }

    /// 解析 old_string，支持精确匹配和模糊容错匹配。
    ///
    /// 返回值：`(effective_old, match_count, was_fuzzy)`
    fn resolve_old_string<'a>(
        old_string: &'a str,
        content: &'a str,
    ) -> Result<(Cow<'a, str>, usize, bool), String> {
        // 1. 精确匹配
        let count = content.matches(old_string).count();
        if count > 0 {
            return Ok((Cow::Borrowed(old_string), count, false));
        }

        // 2. 精确匹配失败，尝试常见的 LLM 复制错误变体
        let mut alternatives: Vec<Cow<'a, str>> = vec![];

        // 先处理 read_file/edit_file 之间的换行符差异（更精确）：
        // read_file 统一返回 LF，但 edit_file 二进制读取保留原始 CRLF
        if !old_string.contains("\r\n") && content.contains("\r\n") {
            alternatives.push(Cow::Owned(old_string.replace('\n', "\r\n")));
        } else if old_string.contains("\r\n") && !content.contains("\r\n") {
            alternatives.push(Cow::Owned(old_string.replace("\r\n", "\n")));
        }

        alternatives.push(Cow::Borrowed(
            old_string.trim_start_matches('\n').trim_end_matches('\n'),
        ));
        alternatives.push(Cow::Borrowed(old_string.trim_start().trim_end()));

        let mut best: Option<(Cow<'a, str>, usize)> = None;

        for alt in alternatives {
            if alt.is_empty() || alt == old_string {
                continue;
            }
            let alt_count = content.matches(alt.as_ref()).count();
            if alt_count == 0 {
                continue;
            }
            // 优先选择匹配次数少的（最好是唯一匹配）
            best = match best {
                None => Some((alt, alt_count)),
                Some((_, existing)) if alt_count < existing => Some((alt, alt_count)),
                other => other,
            };
            if alt_count == 1 {
                break; // 唯一匹配是最优解
            }
        }

        if let Some((alt, alt_count)) = best {
            return Ok((alt, alt_count, true));
        }

        // 3. 全部失败，生成诊断信息
        Err(Self::diagnose_old_string_not_found(content, old_string))
    }

    /// 当 old_string 完全找不到时，生成诊断提示，帮助定位问题
    fn diagnose_old_string_not_found(content: &str, old_string: &str) -> String {
        let old_lines: Vec<&str> = old_string.lines().filter(|l| !l.trim().is_empty()).collect();
        if old_lines.is_empty() {
            return "old_string 为空或只包含空白字符，请提供有效的替换文本。".to_string();
        }

        let first_line = old_lines[0];
        let content_lines: Vec<&str> = content.lines().collect();

        // 在文件中搜索最相似的行
        let mut best_idx = 0;
        let mut best_score = usize::MAX;

        for (idx, line) in content_lines.iter().enumerate() {
            let dist = levenshtein_distance(first_line, line);
            // 完全包含时给更强信号
            let score = if line.contains(first_line) || first_line.contains(line) {
                dist / 2
            } else {
                dist
            };
            if score < best_score {
                best_score = score;
                best_idx = idx;
            }
        }

        if best_score <= 10 {
            // 足够相似，给出具体提示
            let mut ctx = String::new();
            let start = best_idx.saturating_sub(2);
            let end = (best_idx + 3).min(content_lines.len());
            for i in start..end {
                let marker = if i == best_idx { ">>> " } else { "    " };
                ctx.push_str(&format!("{}{}\n", marker, content_lines[i]));
            }
            format!(
                "文件中第 {} 行附近找到最相似的内容，请核对你的 old_string 是否与文件实际内容一致（注意空格、制表符、换行符）：\n```\n{}```",
                best_idx + 1,
                ctx
            )
        } else {
            // 差距太大，给出通用建议
            "请使用 read_file 工具重新确认文件当前内容，检查 old_string 是否存在拼写错误、多余空格或换行符差异。".to_string()
        }
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

    // ── 容错匹配测试 ──

    #[test]
    fn edit_file_自动去除old_string首尾换行符() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "hello world\n");
        // old_string 首尾多了换行符，但文件中没有
        let result = tb.edit_file("test.txt", "\nhello\n", "hi", false).unwrap();
        assert!(result.contains("-hello"), "diff应包含删除行：\n{result}");
        assert!(result.contains("+hi"), "diff应包含新增行：\n{result}");
        assert!(result.contains("[提示]"), "应提示使用了模糊匹配：\n{result}");
        assert_eq!(read_test_file(&tb, "test.txt"), "hi world\n");
    }

    #[test]
    fn edit_file_自动去除old_string首尾空白() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "hello world\n");
        // old_string 首尾多了空格
        let result = tb.edit_file("test.txt", "  hello  ", "hi", false).unwrap();
        assert!(result.contains("-hello"), "diff应包含删除行：\n{result}");
        assert!(result.contains("+hi"), "diff应包含新增行：\n{result}");
        assert!(result.contains("[提示]"), "应提示使用了模糊匹配：\n{result}");
        assert_eq!(read_test_file(&tb, "test.txt"), "hi world\n");
    }

    #[test]
    fn edit_file_自动处理old_string与文件换行符差异() {
        let (tb, _tmp) = test_toolbox();
        // 文件使用 CRLF，但 old_string 使用 LF（模拟从 read_file 复制的内容）
        let content = "line1\r\nline2\r\nline3\r\n";
        write_test_file(&tb, "crlf.txt", content);
        let result = tb.edit_file("crlf.txt", "line2\n", "LINE2\n", false).unwrap();
        assert!(result.contains("-line2"), "diff应包含删除行：\n{result}");
        assert!(result.contains("+LINE2"), "diff应包含新增行：\n{result}");
        assert!(result.contains("[提示]"), "应提示使用了模糊匹配：\n{result}");
        // 写入后仍应保留原始 CRLF
        let path = tb.workspace_root.join("crlf.txt");
        let actual = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual, "line1\r\nLINE2\r\nline3\r\n", "应保留CRLF行尾");
    }

    #[test]
    fn edit_file_找不到时给出诊断信息() {
        let (tb, _tmp) = test_toolbox();
        write_test_file(&tb, "test.txt", "foo bar baz\n");
        let result = tb.edit_file("test.txt", "qux", "xxx", false).unwrap();
        assert!(result.contains("未找到 old_string"), "应提示未找到：\n{result}");
        // 诊断信息应包含相似内容提示（因为 "qux" 和文件中内容差距较大，
        // 但至少应给出通用建议）
        assert!(
            result.contains("请使用 read_file") || result.contains("请核对你的 old_string"),
            "应给出诊断建议：\n{result}"
        );
    }
}
