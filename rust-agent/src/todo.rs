use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::AgentResult;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoStatus {
    pub fn marker(&self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[>]",
            Self::Completed => "[x]",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub status: TodoStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItemInput {
    pub id: String,
    pub text: String,
    pub status: String,
}

impl TodoItemInput {
    pub fn new(id: &str, text: &str, status: &str) -> Self {
        Self {
            id: id.to_owned(),
            text: text.to_owned(),
            status: status.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TodoManager {
    items: Vec<TodoItem>,
}

impl TodoManager {
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
