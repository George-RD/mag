//! Pure, testable doctor check logic.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum ModelCheckResult {
    Ok { model_size_mb: f64 },
    MissingFiles { missing: Vec<String> },
}

pub fn check_model_dir(model_dir: &Path) -> ModelCheckResult {
    let model_onnx = model_dir.join("model.onnx");
    let tokenizer = model_dir.join("tokenizer.json");

    if model_onnx.exists() && tokenizer.exists() {
        let model_size = std::fs::metadata(&model_onnx).map(|m| m.len()).unwrap_or(0);
        #[allow(clippy::cast_precision_loss)]
        let size_mb = model_size as f64 / (1024.0 * 1024.0);
        ModelCheckResult::Ok {
            model_size_mb: size_mb,
        }
    } else {
        let mut missing = Vec::new();
        if !model_onnx.exists() {
            missing.push("model.onnx".to_string());
        }
        if !tokenizer.exists() {
            missing.push("tokenizer.json".to_string());
        }
        ModelCheckResult::MissingFiles { missing }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CrossEncoderCheckResult {
    Ok { model_size_mb: f64 },
    MissingFiles { missing: Vec<String> },
}

pub fn check_cross_encoder_dir(ce_dir: &Path) -> CrossEncoderCheckResult {
    let ce_model = ce_dir.join("model.onnx");
    let ce_tokenizer = ce_dir.join("tokenizer.json");

    if ce_model.exists() && ce_tokenizer.exists() {
        let model_size = std::fs::metadata(&ce_model).map(|m| m.len()).unwrap_or(0);
        #[allow(clippy::cast_precision_loss)]
        let size_mb = model_size as f64 / (1024.0 * 1024.0);
        CrossEncoderCheckResult::Ok {
            model_size_mb: size_mb,
        }
    } else {
        let mut missing = Vec::new();
        if !ce_model.exists() {
            missing.push("model.onnx".to_string());
        }
        if !ce_tokenizer.exists() {
            missing.push("tokenizer.json".to_string());
        }
        CrossEncoderCheckResult::MissingFiles { missing }
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
            ModelCheckResult::Ok { model_size_mb } => {
                assert!(model_size_mb > 0.0);
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
            ModelCheckResult::MissingFiles { missing } => {
                assert!(missing.contains(&"model.onnx".to_string()));
            }
            other => panic!("expected MissingFiles, got {other:?}"),
        }
    }

    #[test]
    fn check_cross_encoder_dir_finds_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let ce_dir = tmp.path();
        fs::write(ce_dir.join("model.onnx"), "fake ce onnx").unwrap();
        fs::write(ce_dir.join("tokenizer.json"), "fake ce tok").unwrap();

        match check_cross_encoder_dir(ce_dir) {
            CrossEncoderCheckResult::Ok { model_size_mb } => {
                assert!(model_size_mb > 0.0);
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn check_cross_encoder_dir_reports_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let ce_dir = tmp.path();

        match check_cross_encoder_dir(ce_dir) {
            CrossEncoderCheckResult::MissingFiles { missing } => {
                assert!(missing.contains(&"model.onnx".to_string()));
                assert!(missing.contains(&"tokenizer.json".to_string()));
            }
            other => panic!("expected MissingFiles, got {other:?}"),
        }
    }
}
