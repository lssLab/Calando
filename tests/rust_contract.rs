use serde_json::Value;

#[test]
fn reads_python_schema_five_without_discarding_unknown_fields() {
    let source = include_str!("fixtures/python-state-v5.json");
    let state: Value = serde_json::from_str(source).unwrap();
    assert_eq!(state["schema_version"], 5);
    assert_eq!(state["stopped_pids"][0], 42);
    assert_eq!(
        state["recent_incidents"][0]["recovery_policy"],
        "lead-probation"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&serde_json::to_string(&state).unwrap()).unwrap(),
        state
    );
}
