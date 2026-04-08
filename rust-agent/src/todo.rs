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
}

impl TodoItemInput {
    /// 创建一条新的待办事项输入
    ///
    /// # 参数
    /// - `id`: 事项标识
    /// - `text`: 事项描述文本
    /// - `status`: 状态字符串
    pub fn new(id: &str, text: &str, status: &str) -> Self {
        Self {
            id: id.to_owned(),
            text: text.to_owned(),
            status: status.to_owned(),
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
    ///
    /// # 参数
    /// - `items`: 新的待办事项输入列表（从 Claude 的工具参数解析而来）
    ///
    /// # 返回值
    /// 更新后渲染的待办列表文本
    ///
    /// # 使用场景
    /// 在 `tools.rs` 的 `dispatch` 处理 `todo` 工具时调用，
    /// 每次调用都会用新列表完全替换旧列表
    ///
    /// # 运作原理
    /// 1. 校验列表大小不超过 20 条
    /// 2. 逐条验证：id 为空则用序号代替，text 不能为空，status 必须是合法值
    /// 3. 统计 in_progress 的数量，最多只允许 1 条
    /// 4. 替换内部列表并返回渲染结果
    pub fn update(&mut self, items: Vec<TodoItemInput>) -> AgentResult<String> {
        if items.len() > 20 {
            bail!("Max 20 todos allowed");
        }

        let mut validated = Vec::with_capacity(items.len());
        let mut in_progress_count = 0;

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
                "completed" => TodoStatus::Completed,
                other => return Err(anyhow!("Item {item_id}: invalid status '{other}'")),
            };

            validated.push(TodoItem {
                id: item_id,
                text,
                status,
            });
        }

        if in_progress_count > 1 {
            bail!("Only one task can be in_progress at a time");
        }

        self.items = validated;
        Ok(self.render())
    }

    /// 将待办列表渲染为可读的文本格式
    ///
    /// # 返回值
    /// 每行一条事项，格式为 `[状态标记] #id: 描述`，末尾附完成进度统计
    ///
    /// # 使用场景
    /// 在 `update` 方法中调用，将结果返回给 Claude；也在调试时用于展示当前任务列表
    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".to_owned();
        }

        let mut lines = self
            .items
            .iter()
            .map(|item| format!("{} #{}: {}", item.status.marker(), item.id, item.text))
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
