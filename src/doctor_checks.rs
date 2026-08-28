//! Pure, testable doctor check logic.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DirCheckResult {
    Ok { size_mb: f64 },
    MissingFiles { missing: Vec<String> },
}

pub(crate) fn check_model_dir(model_dir: &Path) -> DirCheckResult {
    let model_onnx = model_dir.join("model.onnx");
    let tokenizer = model_dir.join("tokenizer.json");

    if model_onnx.exists() && tokenizer.exists() {
        let model_size = std::fs::metadata(&model_onnx).map(|m| m.len()).unwrap_or(0);
        #[allow(clippy::cast_precision_loss)]
        let size_mb = model_size as f64 / (1024.0 * 1024.0);
        DirCheckResult::Ok { size_mb }
    } else {
        let mut missing = Vec::new();
        if !model_onnx.exists() {
            missing.push("model.onnx".to_string());
        }
        if !tokenizer.exists() {
            missing.push("tokenizer.json".to_string());
        }
        DirCheckResult::MissingFiles { missing }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn check_model_dir_finds_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let model_dir = tmp.path();
        fs::write(model_dir.join("model.onnx"), "fake onnx").unwrap();
        fs::write(model_dir.join("tokenizer.json"), "fake tok").unwrap();

        match check_model_dir(model_dir) {
            DirCheckResult::Ok { size_mb } => {
                assert!(size_mb > 0.0);
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn check_model_dir_reports_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let model_dir = tmp.path();
        fs::write(model_dir.join("tokenizer.json"), "fake tok").unwrap();

        match check_model_dir(model_dir) {
            DirCheckResult::MissingFiles { missing } => {
                assert!(missing.contains(&"model.onnx".to_string()));
            }
            other => panic!("expected MissingFiles, got {other:?}"),
        }
    }

    #[test]
    fn check_model_dir_reports_all_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let model_dir = tmp.path();

        match check_model_dir(model_dir) {
            DirCheckResult::MissingFiles { missing } => {
                assert!(missing.contains(&"model.onnx".to_string()));
                assert!(missing.contains(&"tokenizer.json".to_string()));
            }
            other => panic!("expected MissingFiles, got {other:?}"),
        }
    }

    #[test]
    fn post_fix_pass_disables_a_second_fix_attempt() {
        let initial = DoctorRunState::Initial;
        assert!(initial.allows_fixes());

        let recheck = initial.after_fixes();
        assert_eq!(recheck, DoctorRunState::PostFix);
        assert!(!recheck.allows_fixes());
    }
}
