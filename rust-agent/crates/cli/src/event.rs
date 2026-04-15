/// UI 事件（预留，后续扩展使用）
#[allow(dead_code)]
pub enum AppEvent {
    /// 键盘按键
    Key(crossterm::event::KeyEvent),
    /// Agent 事件（文本增量、工具调用等）
    Agent(rust_agent_core::agent::AgentEvent),
    /// Agent 回合完成
    AgentDone(Result<String, anyhow::Error>),
    /// 终端 resize
    Resize(u16, u16),
}
