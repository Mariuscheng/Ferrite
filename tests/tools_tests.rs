use ferrite::tools::{ToolEventSink, ToolName, ToolRegistry};
use std::sync::Arc;

#[test]
fn path_traversal_blocked() {
    let result =
        ToolRegistry::resolve_workspace_path("/tmp/workspace", "../etc/passwd", "test");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("試圖存取工作區以外的檔案"), "got: {}", err);
}

#[test]
fn path_traversal_in_subdir_blocked() {
    let result = ToolRegistry::resolve_workspace_path(
        "/tmp/workspace",
        "subdir/../../../etc/passwd",
        "test",
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("試圖存取工作區以外的檔案"), "got: {}", err);
}

#[test]
fn allowed_path_inside_workspace() {
    let result = ToolRegistry::resolve_workspace_path(".", "src/main.rs", "test");
    assert!(result.is_ok());
}

#[test]
fn command_execution_streams_tool_events() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);
    let sink: ToolEventSink = Arc::new(move |event| {
        events_clone.lock().unwrap().push(event);
    });

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();

    let registry = ToolRegistry::with_event_sink(sink);

    runtime.block_on(async {
        let result = registry
            .execute(
                ToolName::ExecuteCommand,
                serde_json::json!({"command": if cfg!(windows) { "echo hello" } else { "echo hello" }}),
                ".",
            )
            .await;

        assert!(result.success, "command should succeed: {:?}", result.error);
        assert!(
            result.content.contains("hello"),
            "output should contain 'hello'"
        );

        let captured = events.lock().unwrap().clone();
        assert!(!captured.is_empty(), "should have emitted events");

        let start_event = captured.iter().find(|e| e.event_type == "toolStart");
        assert!(start_event.is_some(), "should emit toolStart event");

        let complete_event = captured.iter().find(|e| e.event_type == "toolComplete");
        assert!(complete_event.is_some(), "should emit toolComplete event");
        assert_eq!(complete_event.unwrap().success, Some(true));

        let output_event = captured.iter().find(|e| e.event_type == "toolOutput");
        if let Some(ev) = output_event {
            assert!(
                ev.output.as_ref().unwrap().contains("hello"),
                "toolOutput should contain 'hello'"
            );
        }
    });
}
