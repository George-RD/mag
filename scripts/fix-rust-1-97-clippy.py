#!/usr/bin/env python3
"""Apply the Rust 1.97 Option::filter lint fix. Delete before merge."""

from pathlib import Path

path = Path(__file__).resolve().parents[1] / "src/memory_core/storage/sqlite/crud.rs"
text = path.read_text(encoding="utf-8")
old = '''        let event_at_value: Option<String> = referenced_date
            .clone()
            .and_then(|d| if validate_iso8601(&d) { Some(d) } else { None });'''
new = '''        let event_at_value: Option<String> = referenced_date
            .clone()
            .filter(|date| validate_iso8601(date));'''
if old in text:
    path.write_text(text.replace(old, new, 1), encoding="utf-8")
elif new not in text:
    raise SystemExit("manual Option::filter anchor not found")
