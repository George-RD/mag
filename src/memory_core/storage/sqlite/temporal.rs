use chrono::{Datelike, NaiveDate};

/// Result of expanding temporal references from a query string.
#[derive(Debug, Default)]
pub(super) struct TemporalExpansion {
    /// The query with temporal phrases stripped out.
    pub cleaned_query: String,
    /// ISO 8601 lower bound inferred from the temporal phrase.
    pub event_after: Option<String>,
    /// ISO 8601 upper bound inferred from the temporal phrase.
    pub event_before: Option<String>,
}

/// Detects temporal references in a query and converts them to date filters.
///
/// Recognized patterns:
/// - "today", "yesterday"
/// - "this week", "last week", "this month", "last month"
/// - "N days ago", "N weeks ago", "N months ago"
/// - "last N days", "last N weeks", "last N months", "past N days/weeks/months"
///
/// Returns the cleaned query (temporal phrase removed) and inferred date bounds.
/// If no temporal phrase is detected, returns the original query unchanged.
pub(super) fn expand_temporal_query(query: &str, now: &chrono::NaiveDate) -> TemporalExpansion {
    use chrono::Duration;

    let lower = query.to_ascii_lowercase();

    // Try each pattern; first match wins.
    let result = if let Some(idx) = lower.find("yesterday") {
        let cleaned = remove_phrase(query, idx, 9);
        let date = (*now - Duration::days(1)).format("%Y-%m-%d").to_string();
        Some((
            cleaned,
            Some(date.clone()),
            Some(format!("{date}T23:59:59")),
        ))
    } else if let Some(idx) = lower.find("today") {
        let cleaned = remove_phrase(query, idx, 5);
        let date = now.format("%Y-%m-%d").to_string();
        Some((
            cleaned,
            Some(date.clone()),
            Some(format!("{date}T23:59:59")),
        ))
    } else if let Some(re_pos) = lower.find(" ago") {
        // "N days/weeks/months ago"
        let before = &lower[..re_pos];
        let all_words: Vec<&str> = before.split_whitespace().collect();
        if all_words.len() >= 2 {
            let unit = all_words[all_words.len() - 1];
            let num_str = all_words[all_words.len() - 2];
            num_str.parse::<i64>().ok().and_then(|n| {
                let (after, before_date) = compute_ago(now, n, unit)?;
                let phrase = format!("{num_str} {unit} ago");
                let phrase_start = lower.find(&phrase)?;
                let cleaned = remove_phrase(query, phrase_start, phrase.len());
                Some((cleaned, Some(after), Some(before_date)))
            })
        } else {
            None
        }
    } else if let Some(m) = try_prefix_n_unit(&lower, query, "last ", now) {
        Some(m)
    } else if let Some(m) = try_prefix_n_unit(&lower, query, "past ", now) {
        Some(m)
    } else if let Some(idx) = lower.find("last month") {
        let cleaned = remove_phrase(query, idx, 10);
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1).and_then(|first_this| {
            let last_day_prev = first_this - Duration::days(1);
            let first_prev =
                NaiveDate::from_ymd_opt(last_day_prev.year(), last_day_prev.month(), 1)?;
            Some((
                cleaned,
                Some(first_prev.format("%Y-%m-%d").to_string()),
                Some(format!("{}T23:59:59", last_day_prev.format("%Y-%m-%d"))),
            ))
        })
    } else if let Some(idx) = lower.find("last week") {
        let cleaned = remove_phrase(query, idx, 9);
        let weekday = now.weekday().num_days_from_monday();
        let this_monday = *now - Duration::days(weekday as i64);
        let last_monday = this_monday - Duration::days(7);
        let last_sunday = this_monday - Duration::days(1);
        Some((
            cleaned,
            Some(last_monday.format("%Y-%m-%d").to_string()),
            Some(format!("{}T23:59:59", last_sunday.format("%Y-%m-%d"))),
        ))
    } else if let Some(idx) = lower.find("this week") {
        let cleaned = remove_phrase(query, idx, 9);
        let weekday = now.weekday().num_days_from_monday();
        let start = *now - Duration::days(weekday as i64);
        Some((cleaned, Some(start.format("%Y-%m-%d").to_string()), None))
    } else if let Some(idx) = lower.find("this month") {
        let cleaned = remove_phrase(query, idx, 10);
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .map(|start| (cleaned, Some(start.format("%Y-%m-%d").to_string()), None))
    } else {
        None
    };

    match result {
        Some((cleaned, after, before)) => {
            let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
            if trimmed.is_empty() {
                TemporalExpansion {
                    cleaned_query: query.to_string(),
                    event_after: after,
                    event_before: before,
                }
            } else {
                TemporalExpansion {
                    cleaned_query: trimmed,
                    event_after: after,
                    event_before: before,
                }
            }
        }
        None => TemporalExpansion {
            cleaned_query: query.to_string(),
            event_after: None,
            event_before: None,
        },
    }
}

/// Tries to match "last N unit" or "past N unit" at `prefix` position.
fn try_prefix_n_unit(
    lower: &str,
    query: &str,
    prefix: &str,
    now: &chrono::NaiveDate,
) -> Option<(String, Option<String>, Option<String>)> {
    let idx = lower.find(prefix)?;
    let rest = &lower[idx + prefix.len()..];
    let tokens: Vec<&str> = rest.split_whitespace().take(2).collect();
    if tokens.len() < 2 {
        return None;
    }
    let n: i64 = tokens[0].parse().ok()?;
    let unit = tokens[1];
    let (after, before_date) = compute_ago(now, n, unit)?;
    let phrase_len = prefix.len() + tokens[0].len() + 1 + tokens[1].len();
    let cleaned = remove_phrase(query, idx, phrase_len);
    Some((cleaned, Some(after), Some(before_date)))
}

fn remove_phrase(query: &str, start: usize, len: usize) -> String {
    let mut result = String::with_capacity(query.len());
    result.push_str(&query[..start]);
    if start + len < query.len() {
        result.push_str(&query[start + len..]);
    }
    result
}

fn compute_ago(now: &chrono::NaiveDate, n: i64, unit: &str) -> Option<(String, String)> {
    use chrono::Duration;
    let start = match unit.trim_end_matches('s') {
        "day" => *now - Duration::days(n),
        "week" => *now - Duration::weeks(n),
        "month" => {
            // Approximate: subtract n months
            let n_i32 = i32::try_from(n).ok()?;
            #[allow(clippy::cast_sign_loss)]
            let total_months = now.year() * 12 + now.month() as i32 - 1 - n_i32;
            let year = total_months / 12;
            let month = u32::try_from(total_months % 12 + 1).ok()?;
            let day = now.day().min(28); // safe day
            chrono::NaiveDate::from_ymd_opt(year, month, day)?
        }
        _ => return None,
    };
    Some((
        start.format("%Y-%m-%d").to_string(),
        format!("{}T23:59:59", now.format("%Y-%m-%d")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_today() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors today", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-08"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-08T23:59:59"));
    }

    #[test]
    fn temporal_yesterday() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("what happened yesterday", &now);
        assert_eq!(exp.cleaned_query, "what happened");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-07"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-07T23:59:59"));
    }

    #[test]
    fn temporal_n_days_ago() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("decisions 3 days ago", &now);
        assert_eq!(exp.cleaned_query, "decisions");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-05"));
    }

    #[test]
    fn temporal_last_n_days() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("bugs last 7 days", &now);
        assert_eq!(exp.cleaned_query, "bugs");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn temporal_this_week() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(); // Sunday
        let exp = expand_temporal_query("decisions this week", &now);
        assert_eq!(exp.cleaned_query, "decisions");
        // March 8 2026 is Sunday, weekday=6 from Monday. Monday = March 2.
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-02"));
        assert!(exp.event_before.is_none()); // open-ended
    }

    #[test]
    fn temporal_last_week() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors last week", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-23"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-01T23:59:59"));
    }

    #[test]
    fn temporal_this_month() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let exp = expand_temporal_query("lessons this month", &now);
        assert_eq!(exp.cleaned_query, "lessons");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn temporal_last_month() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let exp = expand_temporal_query("bugs last month", &now);
        assert_eq!(exp.cleaned_query, "bugs");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-01"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-02-28T23:59:59"));
    }

    #[test]
    fn temporal_no_match() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("database connection pool", &now);
        assert_eq!(exp.cleaned_query, "database connection pool");
        assert!(exp.event_after.is_none());
        assert!(exp.event_before.is_none());
    }

    #[test]
    fn temporal_query_only_temporal() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("today", &now);
        // When only temporal phrase, keep original query for search
        assert_eq!(exp.cleaned_query, "today");
        assert!(exp.event_after.is_some());
    }

    #[test]
    fn temporal_past_n_weeks() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors past 2 weeks", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-22"));
    }
}
