use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::AgentResult;

/// 待办事项的状态枚举
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    /// 待处理
    Pending,
    /// 进行中
    InProgress,
    /// 已完成
    Completed,
}

impl TodoStatus {
    /// 返回状态对应的可视化标记符号
    ///
    /// # 返回值
    /// - Pending → `[ ]`
    /// - InProgress → `[>]`
    /// - Completed → `[x]`
    ///
    /// # 使用场景
    /// 在 `TodoManager::render` 中渲染每条待办事项时使用
    pub fn marker(&self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[>]",
            Self::Completed => "[x]",
        }
    }
}

/// 单条待办事项
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    /// 事项的唯一标识（编号）
    pub id: String,
    /// 事项的文本描述
    pub text: String,
    /// 事项的当前状态
    pub status: TodoStatus,
    /// 事项的产出摘要（可选）
    pub result_summary: Option<String>,
}

/// 待办事项的输入格式（从 Claude 工具参数 JSON 中解析）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItemInput {
    /// 事项标识
    pub id: String,
    /// 事项描述
    pub text: String,
    /// 状态字符串（"pending"、"in_progress"、"completed"）
    pub status: String,
    /// 任务产出的简要描述或数据预览
    #[serde(default)]
    pub result_summary: Option<String>,
}

impl TodoItemInput {
    /// 创建一条新的待办事项输入
    pub fn new(id: &str, text: &str, status: &str) -> Self {
        Self {
            id: id.to_owned(),
            text: text.to_owned(),
            status: status.to_owned(),
            result_summary: None,
        }
    }
}

/// 待办事项管理器：维护和渲染 Agent 的任务列表
#[derive(Clone, Debug, Default)]
pub struct TodoManager {
    /// 当前所有待办事项
    items: Vec<TodoItem>,
}

impl TodoManager {
    /// 用新的事项列表完全替换当前列表（整体更新）
    pub fn update(&mut self, items: Vec<TodoItemInput>) -> AgentResult<String> {
        if items.len() > 20 {
            bail!("Max 20 todos allowed");
        }

        let mut validated = Vec::with_capacity(items.len());
        let mut in_progress_count = 0;
        let mut completed_items_with_results = Vec::new();

        for (index, item) in items.into_iter().enumerate() {
            let item_id = if item.id.trim().is_empty() {
                (index + 1).to_string()
            } else {
                item.id.trim().to_owned()
            };
            let text = item.text.trim().to_owned();
            if text.is_empty() {
                bail!("Item {item_id}: text required");
            }

            let status = match item.status.trim().to_ascii_lowercase().as_str() {
                "pending" => TodoStatus::Pending,
                "in_progress" => {
                    in_progress_count += 1;
                    TodoStatus::InProgress
                }
                "completed" => {
                    if let Some(ref res) = item.result_summary {
                        completed_items_with_results.push((item_id.clone(), res.clone()));
                    }
                    TodoStatus::Completed
                }
                other => return Err(anyhow!("Item {item_id}: invalid status '{other}'")),
            };

            validated.push(TodoItem {
                id: item_id,
                text,
                status,
                result_summary: item.result_summary,
            });
        }

        if in_progress_count > 1 {
            bail!("Only one task can be in_progress at a time");
        }

        self.items = validated;
        
        let mut output = self.render();
        if !completed_items_with_results.is_empty() {
            output.push_str("\n\n⚠️ 确认提醒：");
            for (id, res) in completed_items_with_results {
                output.push_str(&format!("\n任务 #{} 已标记为完成。你必须在当前的回复中向用户展示该步骤的关键结果（摘要：{}），严禁只说‘已完成’。", id, res));
            }
        }
        
        Ok(output)
    }

    /// 将待办列表渲染为可读的文本格式
    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".to_owned();
        }

        let mut lines = self
            .items
            .iter()
            .map(|item| {
                let base = format!("{} #{}: {}", item.status.marker(), item.id, item.text);
                if let Some(ref res) = item.result_summary {
                    format!("{} [结果: {}]", base, res)
                } else {
                    base
                }
            })
            .collect::<Vec<_>>();
        let done = self
            .items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        lines.push(format!("\n({done}/{}) completed)", self.items.len()));
        lines.join("\n")
    }
}
