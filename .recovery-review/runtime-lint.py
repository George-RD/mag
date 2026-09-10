from pathlib import Path
p=Path('benches/runtime_behaviour/main.rs'); s=p.read_text().replace('args.dataset == PathBuf::from(DEFAULT_DATASET_DIR)', 'args.dataset.as_path() == std::path::Path::new(DEFAULT_DATASET_DIR)'); p.write_text(s)
p=Path('benches/runtime_behaviour/dataset/validation.rs'); s=p.read_text(); old='''        if let Some(offset) = seed.day_offset {
            if chrono::Duration::try_days(offset)
                .and_then(|d| today.checked_add_signed(d))
                .is_none()
            {
                failures.push(format!(
                    "seed {} has an unrepresentable day offset",
                    seed.key
                ));
            }
        }'''; new='''        if let Some(offset) = seed.day_offset
            && chrono::Duration::try_days(offset)
                .and_then(|d| today.checked_add_signed(d))
                .is_none()
        {
            failures.push(format!(
                "seed {} has an unrepresentable day offset",
                seed.key
            ));
        }'''; assert old in s; p.write_text(s.replace(old,new))
p=Path('meta/reviews/runtime-behaviour-recovery.md'); s=p.read_text().replace('actual custom path', 'sanitized custom-source identity'); s='''---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-10
reviewer: OpenAI coding assistant
---
'''+s; s+='''\nRun 34463306071 passes the recovered unit and nine process tests under both\nno-default-features and the production embedding feature. Its strict Clippy\ngate then identifies a nested date check and an allocated comparison path. Both\nare simplified without suppressing lint or changing the checked date bounds.\n'''; p.write_text(s)
