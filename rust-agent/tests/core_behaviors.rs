use rust_agent::skills::parse_skill_file;
use rust_agent::todo::{TodoItemInput, TodoManager};
use rust_agent::workspace::resolve_workspace_path;

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
fn resolve_workspace_path_rejects_escape() {
    let root = std::env::current_dir().unwrap().join("fixtures");
    let err = resolve_workspace_path(&root, "..\\outside.txt").unwrap_err();

    assert!(err.to_string().contains("Path escapes workspace"));
}
