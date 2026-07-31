#[test]
fn lessons_command_routes_through_local_runtime() {
    let main_source = include_str!("../src/main.rs");
    let compact_source: String = main_source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    assert!(
        compact_source.contains("local_runtime.query_lessons("),
        "lessons still bypasses the selected local runtime"
    );
    assert!(
        main_source.contains("Lessons queried through local memory runtime"),
        "lessons does not report the selected runtime path"
    );
}
