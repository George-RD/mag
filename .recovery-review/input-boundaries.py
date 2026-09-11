from pathlib import Path
import sys

phase = sys.argv[1]
if phase == 'tests':
    p = Path('tests/runtime_behaviour_eval.rs')
    p.write_text(p.read_text() + r'''
fn assert_boundary_rejected(output: Output, diagnostic: &str) {
    assert!(!output.status.success(), "invalid observation boundary was accepted");
    assert!(String::from_utf8_lossy(&output.stdout).contains(diagnostic));
}

#[test]
fn input_boundary_rejects_vacuous_temporal_case() {
    assert_boundary_rejected(mutated_validation(|data| {
        data["temporal"][0]["expect_keys"] = json!([]);
        data["temporal"][0]["expect_absent_keys"] = json!([]);
    }), "no falsifiable temporal expectations");
}

#[test]
fn input_boundary_rejects_relationship_weight_above_one() {
    assert_boundary_rejected(mutated_validation(|data| {
        data["relationships"][0]["min_weight"] = json!(1.01);
    }), "relationship weight must be between 0 and 1");
}

#[test]
fn input_boundary_rejects_relationship_weight_below_zero() {
    assert_boundary_rejected(mutated_validation(|data| {
        data["relationships"][0]["min_weight"] = json!(-0.01);
    }), "relationship weight must be between 0 and 1");
}

fn grouping_indices(data: &Value) -> Vec<usize> {
    data["seed"].as_array().unwrap().iter().enumerate()
        .filter(|(_, seed)| seed["group"] == "grouping")
        .map(|(index, _)| index).collect()
}

#[test]
fn input_boundary_rejects_duplicate_grouping_content() {
    assert_boundary_rejected(mutated_validation(|data| {
        let indices = grouping_indices(data);
        data["seed"][indices[1]]["content"] = data["seed"][indices[0]]["content"].clone();
    }), "duplicate grouping content");
}

#[test]
fn input_boundary_rejects_padded_grouping_content() {
    assert_boundary_rejected(mutated_validation(|data| {
        let index = grouping_indices(data)[0];
        let content = data["seed"][index]["content"].as_str().unwrap();
        data["seed"][index]["content"] = json!(format!("  {content}  "));
    }), "ambiguous grouping content");
}

#[test]
fn input_boundary_rejects_embedded_grouping_separator() {
    assert_boundary_rejected(mutated_validation(|data| {
        let index = grouping_indices(data)[0];
        let content = data["seed"][index]["content"].as_str().unwrap();
        data["seed"][index]["content"] = json!(format!("{content}\n---\nextra segment"));
    }), "ambiguous grouping content");
}

#[test]
fn relationship_weight_boundary_values_are_supported() {
    for weight in [0.0, 1.0] {
        let output = mutated_validation(|data| {
            data["relationships"][0]["min_weight"] = json!(weight);
        });
        assert!(output.status.success(), "valid endpoint weight was rejected");
    }
}
''')
elif phase == 'apply':
    p = Path('benches/runtime_behaviour/dataset/validation.rs')
    text = p.read_text()
    needle = '    for case in &data.temporal {\n        let expected:'
    assert text.count(needle) == 1
    text = text.replace(needle, '''    for case in &data.temporal {
        if case.expect_keys.is_empty() && case.expect_absent_keys.is_empty() {
            failures.push(format!("temporal {} has no falsifiable temporal expectations", case.id));
        }
        let expected:''')
    needle = '    let mut grouping_members = BTreeSet::new();'
    assert text.count(needle) == 1
    guard = '''    for case in &data.relationships {
        if !(0.0..=1.0).contains(&case.min_weight) {
            failures.push(format!("{} -> {}: relationship weight must be between 0 and 1", case.from, case.to));
        }
    }
    let mut grouping_content = BTreeSet::new();
    for seed in data.seed.iter().filter(|seed| seed.group == "grouping") {
        if seed.content.trim().is_empty()
            || seed.content != seed.content.trim()
            || seed.content.contains("\\n---\\n")
        {
            failures.push(format!("seed {} has ambiguous grouping content", seed.key));
        }
        if !grouping_content.insert(seed.content.as_str()) {
            failures.push(format!("seed {} has duplicate grouping content", seed.key));
        }
    }
'''
    p.write_text(text.replace(needle, guard + needle))
    p = Path('benches/runtime_behaviour/README.md')
    p.write_text(p.read_text() + '''

Custom temporal cases must declare a present or absent expectation. Relationship
minimum weights use the production range 0 through 1, including both endpoints.
Grouping membership is reconstructed from compacted content, so grouping seed
content must be nonempty, unique, trimmed and free of the compact separator
`\\n---\\n`. Unsupported inputs fail validation rather than receive misleading
scores. These restrictions do not change the preserved corpus or production API.
''')
else:
    raise SystemExit('use tests or apply')
