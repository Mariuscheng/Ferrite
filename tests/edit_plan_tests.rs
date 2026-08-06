use ferrite::edit_plan::ValidationRunResult;

#[test]
fn validation_run_result_serializes() {
    let result = ValidationRunResult {
        command: "cargo build".into(),
        success: true,
        output: "Compiled successfully".into(),
        error: None,
    };
    let json = serde_json::to_value(&result).expect("serialize");
    assert_eq!(json["command"], "cargo build");
    assert_eq!(json["success"], true);
}
