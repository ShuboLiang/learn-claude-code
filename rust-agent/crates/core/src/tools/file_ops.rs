use anyhow::Context;

use crate::AgentResult;
use crate::infra::utils::truncate_text;
use crate::infra::workspace::resolve_workspace_path;

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

    /// 在文件中精确替换一段文本（首次出现的位置）
    pub(crate) fn edit_file(
        &self,
        path: &str,
        old_text: &str,
        new_text: &str,
    ) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, path)?;
        let content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read {}", resolved.display()))?;
        if !content.contains(old_text) {
            return Ok(format!("错误：在 {path} 中未找到指定文本"));
        }
        let updated = content.replacen(old_text, new_text, 1);
        std::fs::write(&resolved, updated)
            .with_context(|| format!("Failed to write {}", resolved.display()))?;
        Ok(format!("已编辑 {path}"))
    }
}
