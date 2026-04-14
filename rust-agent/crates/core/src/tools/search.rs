use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::AgentResult;
use crate::infra::workspace::resolve_workspace_path;
use crate::infra::utils::truncate_text;

/// 需要跳过的大目录列表
const SKIP_DIRS: &[&str] = &[
    "target", "node_modules", ".git", ".svn", ".hg",
    "__pycache__", ".next", ".nuxt", "dist", "build",
    ".cache", ".venv", "venv",
];

impl super::AgentToolbox {
    /// 使用 glob 模式搜索匹配的文件路径
    pub(crate) fn glob_search(&self, pattern: &str, path: Option<&str>) -> AgentResult<String> {
        let base = match path {
            Some(p) => resolve_workspace_path(&self.workspace_root, p)?,
            None => self.workspace_root.clone(),
        };

        if !base.is_dir() {
            return Ok(format!("错误：{} 不是目录", base.display()));
        }

        let full_pattern = base.join(pattern);
        let full_pattern_str = full_pattern.to_string_lossy();

        let mut results = Vec::new();
        for entry in glob::glob(&full_pattern_str)
            .with_context(|| format!("无效的 glob 模式: {pattern}"))?
            .filter_map(Result::ok)
        {
            if should_skip_path(&entry, SKIP_DIRS) {
                continue;
            }
            if !entry.is_file() {
                continue;
            }

            if let Ok(rel) = entry.strip_prefix(&self.workspace_root) {
                results.push(rel.to_string_lossy().into_owned());
            }

            if results.len() >= 250 {
                break;
            }
        }

        if results.is_empty() {
            return Ok("（无匹配文件）".to_owned());
        }

        Ok(results.join("\n"))
    }

    /// 在文件内容中搜索匹配正则表达式的行
    pub(crate) fn grep_search(
        &self,
        pattern: &str,
        path: Option<&str>,
        glob_filter: Option<&str>,
        output_mode: Option<&str>,
        case_insensitive: bool,
        context_lines: Option<usize>,
        head_limit: Option<usize>,
    ) -> AgentResult<String> {
        let mode = output_mode.unwrap_or("files_with_matches");
        let limit = head_limit.unwrap_or(250);

        let base = match path {
            Some(p) => resolve_workspace_path(&self.workspace_root, p)?,
            None => self.workspace_root.clone(),
        };

        // 构建正则表达式
        let mut regex_builder = regex::RegexBuilder::new(pattern);
        regex_builder.case_insensitive(case_insensitive);
        let re = regex_builder
            .build()
            .with_context(|| format!("无效的正则表达式: {pattern}"))?;

        // 收集要搜索的文件列表
        let files = collect_search_files(&base, glob_filter, &self.workspace_root)?;
        if files.is_empty() {
            return Ok("（无文件可搜索）".to_owned());
        }

        // 三种模式各自就地构建输出，避免二次遍历
        let mut output_lines: Vec<String> = Vec::new();
        let mut file_count = 0;

        for file_path in &files {
            if file_count >= limit {
                break;
            }

            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let lines: Vec<&str> = content.lines().collect();

            // 找到所有匹配行的索引（0-based）
            let matched_indices: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| re.is_match(line))
                .map(|(i, _)| i)
                .collect();

            if matched_indices.is_empty() {
                continue;
            }

            let rel_path = file_path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(file_path)
                .to_string_lossy();

            match mode {
                "files_with_matches" => {
                    output_lines.push(rel_path.into_owned());
                    file_count += 1;
                }
                "count" => {
                    output_lines.push(format!("{}: {}", rel_path, matched_indices.len()));
                    file_count += 1;
                }
                "content" => {
                    let ctx = context_lines.unwrap_or(0);

                    if ctx > 0 {
                        // 带上下文行：收集需要显示的行号，合并相邻区间
                        let mut show_ranges: Vec<(usize, usize)> = Vec::new();
                        for &idx in &matched_indices {
                            let start = idx.saturating_sub(ctx);
                            let end = (idx + ctx + 1).min(lines.len());
                            show_ranges.push((start, end));
                        }
                        // 合并重叠区间
                        show_ranges.sort();
                        let mut merged: Vec<(usize, usize)> = Vec::new();
                        for range in show_ranges {
                            if let Some(last) = merged.last_mut() {
                                if range.0 <= last.1 {
                                    last.1 = last.1.max(range.1);
                                    continue;
                                }
                            }
                            merged.push(range);
                        }

                        output_lines.push(format!("{}:", rel_path));
                        for (start, end) in merged {
                            for i in start..end {
                                let line_num = i + 1;
                                let marker =
                                    if matched_indices.contains(&i) { ">" } else { " " };
                                output_lines.push(format!(
                                    "{marker} {line_num:4} | {}",
                                    lines[i]
                                ));
                                file_count += 1;
                                if file_count >= limit {
                                    break;
                                }
                            }
                        }
                    } else {
                        // 无上下文，只显示匹配行
                        for &idx in &matched_indices {
                            output_lines.push(format!(
                                "  {}:{}: {}",
                                rel_path,
                                idx + 1,
                                lines[idx]
                            ));
                            file_count += 1;
                            if file_count >= limit {
                                break;
                            }
                        }
                    }
                }
                _ => return Ok(format!("未知输出模式: {mode}")),
            }
        }

        if output_lines.is_empty() {
            return Ok("（无匹配结果）".to_owned());
        }

        let total = output_lines.len();
        let output = output_lines.into_iter().take(limit).collect::<Vec<_>>().join("\n");
        let suffix = if total > limit {
            format!("\n... （共 {total} 条结果，仅显示前 {limit} 条）")
        } else {
            String::new()
        };

        Ok(truncate_text(&format!("{output}{suffix}"), 50_000))
    }
}

/// 收集指定路径下需要搜索的文件列表
fn collect_search_files(
    base: &Path,
    glob_filter: Option<&str>,
    _workspace_root: &Path,
) -> AgentResult<Vec<PathBuf>> {
    if base.is_file() {
        return Ok(vec![base.to_path_buf()]);
    }

    let pattern = glob_filter.unwrap_or("**/*");
    let full_pattern = base.join(pattern);
    let full_pattern_str = full_pattern.to_string_lossy();

    let files: Vec<PathBuf> = glob::glob(&full_pattern_str)
        .with_context(|| format!("无效的 glob 模式: {pattern}"))?
        .filter_map(Result::ok)
        .filter(|p| !should_skip_path(p, SKIP_DIRS))
        .filter(|p| p.is_file())
        .collect();

    Ok(files)
}

/// 判断路径是否应该被跳过（属于无关的大目录）
fn should_skip_path(path: &Path, skip_dir_names: &[&str]) -> bool {
    path.components().any(|comp| {
        comp.as_os_str()
            .to_str()
            .is_some_and(|name| skip_dir_names.contains(&name))
    })
}
