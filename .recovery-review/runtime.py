from pathlib import Path
import subprocess

p = Path('tests/runtime_behaviour_eval.rs')
s = p.read_text().replace('preserves_original_dataset_identity_and_declared_path', 'preserves_original_dataset_identity_and_sanitized_path')
s = s.replace('''    assert!(
        output["metadata"]["dataset_path"]
            .as_str()
            .unwrap()
            .ends_with("data/runtime_behaviour_eval/v1/dataset.json")
    );''', '''    assert_eq!(output["metadata"]["dataset_path"], "dataset.json");
    assert_eq!(output["metadata"]["dataset_source"], "repo-local");''')
s = s.replace('reports_actual_custom_dataset_path', 'reports_custom_dataset_source_without_disclosing_local_path')
s = s.replace('''    assert_eq!(
        result["metadata"]["dataset_path"],
        temp.path().join("dataset.json").to_str().unwrap()
    );''', '''    assert_eq!(result["metadata"]["dataset_path"], "dataset.json");
    assert_eq!(result["metadata"]["dataset_source"], "user-supplied");
    assert!(!result.to_string().contains(temp.path().to_str().unwrap()));''')
assert 'user-supplied' in s and 'reports_actual_custom_dataset_path' not in s
p.write_text(s)
red = subprocess.run(['cargo','test','--locked','--no-default-features','--test','runtime_behaviour_eval','reports_custom_dataset_source_without_disclosing_local_path','--','--exact'], capture_output=True,text=True)
print((red.stdout + red.stderr)[-6000:])
assert red.returncode != 0 and 'left: String("repo-local")' in red.stdout and 'right: "user-supplied"' in red.stdout
p = Path('benches/runtime_behaviour/main.rs'); s = p.read_text()
s = s.replace('const ALL_FAMILIES:', 'const DEFAULT_DATASET_DIR: &str = "data/runtime_behaviour_eval/v1";\nconst ALL_FAMILIES:')
s = s.replace('default_value = "data/runtime_behaviour_eval/v1"', 'default_value = DEFAULT_DATASET_DIR')
s = s.replace('''        "memory_runtime_eval",
        "repo-local",''', '''        "memory_runtime_eval",
        if args.dataset == PathBuf::from(DEFAULT_DATASET_DIR) {
            "repo-local"
        } else {
            "user-supplied"
        },''')
p.write_text(s)
p = Path('benches/runtime_behaviour/README.md'); p.write_text(p.read_text().replace('Metadata records the actual supplied dataset path and hash.', 'Metadata retains the shared path sanitization, records the dataset hash, and\ndistinguishes the default repository corpus from user-supplied directories.\nLocal directory names are not disclosed in the dataset-path field.'))
p = Path('meta/reviews/runtime-behaviour-recovery.md'); p.write_text(p.read_text() + '''\n## Metadata regression diagnosis

Run 34462869362 reproduces two test-contract failures at c347eb3: the shared
benchmark metadata helper intentionally reduces dataset paths to their filename.
Preserve that privacy behavior instead of overriding it. Corrected tests retain
the original digest assertion and add a custom-source/no-local-path check. That
new assertion fails first because custom data is incorrectly labelled repo-local;
the diagnostic now distinguishes user-supplied directories. No shared production
helper, extraction scorer or dataset byte is changed.
''')
