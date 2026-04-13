use std::path::PathBuf;

/// 列出已安装技能目录的文件树结构
///
/// 在所有技能目录中查找，后加载的优先（项目目录覆盖用户目录）。
/// 返回目录树文本，让 Claude 了解技能的完整文件结构并自行判断调用方式。
pub(crate) fn list_skill_tree(skill_name: &str, skill_dirs: &[PathBuf]) -> String {
    let skill_dir = skill_dirs
        .iter()
        .rev()
        .map(|d| d.join(skill_name))
        .find(|d| d.exists());

    let skill_dir = match skill_dir {
        Some(d) => d,
        None => return String::new(),
    };

    let mut lines = vec![format!("技能目录结构 ({}):", skill_dir.display())];
    for entry in walkdir::WalkDir::new(&skill_dir)
        .sort_by(|a, b| a.file_name().cmp(b.file_name()))
        .into_iter()
        .filter_map(Result::ok)
    {
        let depth = entry.depth();
        let indent = "  ".repeat(depth);
        let name = entry.file_name().to_string_lossy();
        if depth == 0 {
            lines.push(format!("{indent}{name}/"));
        } else if entry.file_type().is_dir() {
            lines.push(format!("{indent}{name}/"));
        } else {
            lines.push(format!("{indent}{name}"));
        }
    }

    lines.join("\n")
}
