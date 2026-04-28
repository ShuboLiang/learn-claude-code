use rust_agent_core::bots::BotRegistry;
use rust_agent_core::infra::todo::{TodoItemInput, TodoManager};
use rust_agent_core::infra::workspace::resolve_workspace_path;
use rust_agent_core::skills::parse_skill_file;
use rust_agent_core::ContextService;

#[test]
fn todo_manager_rejects_multiple_in_progress_items() {
    let mut manager = TodoManager::default();
    let err = manager
        .update(vec![
            TodoItemInput::new("1", "first", "in_progress"),
            TodoItemInput::new("2", "second", "in_progress"),
        ])
        .unwrap_err();

    assert!(err.to_string().contains("Only one task can be in_progress"));
}

#[test]
fn parse_skill_file_reads_frontmatter_and_body() {
    let raw = "---\nname: pdf\ndescription: Process PDFs\n---\nStep 1\nStep 2\n";
    let parsed = parse_skill_file(raw).unwrap();

    assert_eq!(parsed.metadata.name.as_deref(), Some("pdf"));
    assert_eq!(parsed.metadata.description.as_deref(), Some("Process PDFs"));
    assert_eq!(parsed.body, "Step 1\nStep 2");
}

#[test]
fn resolve_workspace_path_resolves_outside_path() {
    // 路径不再受工作区限制，相对路径会正常解析
    let root = std::env::current_dir().unwrap();
    let result = resolve_workspace_path(&root, "../outside.txt");
    assert!(result.is_ok());
}

// ── Bot 会话管理测试 ──

#[test]
fn bot_session_save_and_retrieve() {
    let registry = BotRegistry::default();

    // 首次获取 — 应无活跃会话
    assert!(registry.get_session("resume-screener").is_none());

    // 保存会话
    let ctx = ContextService::new();
    registry.save_session("resume-screener".to_owned(), ctx);

    // 再次获取 — 应有活跃会话
    let session = registry.get_session("resume-screener");
    assert!(session.is_some());
    assert!(!session.unwrap().is_expired());
}

#[test]
fn bot_session_clear_removes_session() {
    let registry = BotRegistry::default();

    // 保存会话
    let ctx = ContextService::new();
    registry.save_session("resume-screener".to_owned(), ctx);
    assert!(registry.get_session("resume-screener").is_some());

    // 清除会话
    registry.clear_session("resume-screener");
    assert!(registry.get_session("resume-screener").is_none());
}

#[test]
fn bot_session_cleanup_preserves_fresh_sessions() {
    let registry = BotRegistry::default();

    // 保存刚创建的会话
    let ctx = ContextService::new();
    registry.save_session("test-bot".to_owned(), ctx);

    // 清理过期会话 — 刚创建的会话不应被删除
    registry.cleanup_expired_sessions();
    assert!(registry.get_session("test-bot").is_some());
}

#[test]
fn bot_session_save_overwrites_previous() {
    let registry = BotRegistry::default();

    // 第一次保存
    let ctx1 = ContextService::new();
    registry.save_session("repeated-bot".to_owned(), ctx1);

    // 第二次保存（覆盖）
    let ctx2 = ContextService::new();
    registry.save_session("repeated-bot".to_owned(), ctx2);

    // 应仍有会话（被覆盖而不是丢失）
    assert!(registry.get_session("repeated-bot").is_some());
}
