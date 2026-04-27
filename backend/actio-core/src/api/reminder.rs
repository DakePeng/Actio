use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::session::AppApiError;
use crate::domain::types::{
    NewReminder, PatchReminderRequest, Reminder, ReminderFilter, ReminderTrace, ReminderTraceLine,
};
use crate::repository::label as label_repo;
use crate::repository::reminder as reminder_repo;
use crate::AppState;

// ── Relative date/time resolver ─────────────────────────────────────────

/// Resolve relative date/time references in user input to a `NaiveDateTime`
/// (local time). Returns `None` if no recognizable time reference is found.
///
/// This replaces/overrides the LLM's date math which is unreliable on small
/// models. Handles: today, tonight, tomorrow, day after tomorrow, weekday
/// names, "next Monday", "this weekend", "in N hours/days/minutes", "end of
/// day/week", bare times like "at 10", "3pm", "noon".
fn resolve_relative_datetime(input: &str) -> Option<NaiveDateTime> {
    let lower = input.to_lowercase();
    let now = chrono::Local::now().naive_local();
    let today = now.date();

    // Explicit rejection: we don't schedule anything in the past.
    if contains_word(&lower, "yesterday") || input.contains("昨天") {
        return None;
    }

    // ── Day resolution (order matters — longer phrases first) ──
    let day = if lower.contains("day after tomorrow") {
        Some(today + Duration::days(2))
    } else if lower.contains("tomorrow") {
        Some(today + Duration::days(1))
    } else if lower.contains("tonight") || lower.contains("today") || lower.contains("this evening")
    {
        Some(today)
    } else if lower.contains("this weekend") {
        Some(next_weekday(today, Weekday::Sat))
    } else if lower.contains("next week") {
        Some(next_weekday(today, Weekday::Mon) + Duration::weeks(1))
    } else if lower.contains("next month") {
        Some(first_of_next_month(today))
    } else if lower.contains("end of month") || lower.contains("eom") {
        Some(last_of_month(today))
    } else if lower.contains("end of week") || lower.contains("eow") {
        Some(next_weekday(today, Weekday::Fri))
    } else if lower.contains("end of day") || lower.contains("eod") {
        Some(today)
    } else if let Some(wd) = parse_weekday_ref(&lower) {
        Some(next_weekday(today, wd))
    } else if let Some(d) = parse_absolute_date(input, today) {
        Some(d)
    } else {
        None
    };

    // ── "in N <unit>" patterns ──
    // If the user ALSO specified an explicit time-of-day ("in 2 days at 5pm"),
    // combine the day delta with that time instead of dropping it.
    if let Some(dt) = parse_in_duration(&lower, now) {
        if let Some(t) = parse_time_of_day(&lower) {
            return Some(dt.date().and_time(t));
        }
        return Some(dt);
    }

    // ── Combine day + time ──
    if let Some(d) = day {
        let time = parse_time_of_day(&lower).unwrap_or_else(|| {
            if lower.contains("tonight") || lower.contains("this evening") {
                NaiveTime::from_hms_opt(20, 0, 0).unwrap()
            } else if lower.contains("end of day") || lower.contains("eod") {
                NaiveTime::from_hms_opt(17, 0, 0).unwrap()
            } else {
                NaiveTime::from_hms_opt(9, 0, 0).unwrap()
            }
        });
        return Some(d.and_time(time));
    }

    // ── Bare time reference (implies the nearest future occurrence) ──
    parse_time_of_day(&lower).map(|t| {
        // If the resolved time has already passed today, roll to tomorrow.
        if today.and_time(t) <= now {
            (today + Duration::days(1)).and_time(t)
        } else {
            today.and_time(t)
        }
    })
}

/// First day of the month after `from`.
fn first_of_next_month(from: NaiveDate) -> NaiveDate {
    let (y, m) = if from.month() == 12 {
        (from.year() + 1, 1)
    } else {
        (from.year(), from.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(from)
}

/// Last day of the current month.
fn last_of_month(from: NaiveDate) -> NaiveDate {
    let first_next = first_of_next_month(from);
    first_next - Duration::days(1)
}

/// True iff `needle` appears in `haystack` surrounded by non-alphabetic chars
/// (i.e. matches on a word boundary). `haystack` is expected to be ASCII-lower
/// where boundary matters; the helper is tolerant of non-ASCII by treating any
/// byte that isn't an ASCII letter as a boundary.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    let mut i = 0;
    while i + nb.len() <= hb.len() {
        if &hb[i..i + nb.len()] == nb {
            let before_ok = i == 0 || !hb[i - 1].is_ascii_alphabetic();
            let after_idx = i + nb.len();
            let after_ok = after_idx == hb.len() || !hb[after_idx].is_ascii_alphabetic();
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Find the next occurrence of `target` weekday strictly after `from`.
fn next_weekday(from: NaiveDate, target: Weekday) -> NaiveDate {
    let from_wd = from.weekday().num_days_from_monday() as i64;
    let target_wd = target.num_days_from_monday() as i64;
    let mut diff = target_wd - from_wd;
    if diff <= 0 {
        diff += 7;
    }
    from + Duration::days(diff)
}

/// Parse weekday name references: "monday", "next tuesday", "this fri", etc.
fn parse_weekday_ref(input: &str) -> Option<Weekday> {
    let pairs = [
        ("monday", Weekday::Mon),
        ("mon ", Weekday::Mon),
        ("mon,", Weekday::Mon),
        ("tuesday", Weekday::Tue),
        ("tues", Weekday::Tue),
        ("tue ", Weekday::Tue),
        ("wednesday", Weekday::Wed),
        ("wed ", Weekday::Wed),
        ("wed,", Weekday::Wed),
        ("thursday", Weekday::Thu),
        ("thurs", Weekday::Thu),
        ("thu ", Weekday::Thu),
        ("friday", Weekday::Fri),
        ("fri ", Weekday::Fri),
        ("fri,", Weekday::Fri),
        ("saturday", Weekday::Sat),
        ("sat ", Weekday::Sat),
        ("sat,", Weekday::Sat),
        ("sunday", Weekday::Sun),
        ("sun ", Weekday::Sun),
        ("sun,", Weekday::Sun),
    ];
    // Pad input so partial matches at the end still work
    let padded = format!("{input} ");
    for (name, wd) in &pairs {
        if padded.contains(name) {
            return Some(*wd);
        }
    }
    None
}

/// Parse "in N minutes/hours/days/weeks" patterns.
fn parse_in_duration(input: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    // "in an hour", "in a week"
    if input.contains("in an hour") || input.contains("in 1 hour") {
        return Some(now + Duration::hours(1));
    }
    if input.contains("in a week") || input.contains("in 1 week") {
        return Some(now + Duration::weeks(1));
    }

    // "in N <unit>"
    let re_patterns: &[(&str, fn(i64) -> Duration)] = &[
        ("min", Duration::minutes),
        ("hour", Duration::hours),
        ("day", Duration::days),
        ("week", Duration::weeks),
    ];
    if let Some(pos) = input.find("in ") {
        let after = &input[pos + 3..];
        let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = num_str.parse::<i64>() {
            let rest = after[num_str.len()..].trim_start();
            for (unit, to_dur) in re_patterns {
                if rest.starts_with(unit) {
                    return Some(now + to_dur(n));
                }
            }
        }
    }

    None
}

/// Parse explicit time-of-day from input. Handles:
/// "at 10", "at 10am", "at 3:30pm", "at 15:00", "3pm", "noon", "midnight"
fn parse_time_of_day(input: &str) -> Option<NaiveTime> {
    // Named times
    if input.contains("noon") {
        return NaiveTime::from_hms_opt(12, 0, 0);
    }
    if input.contains("midnight") {
        return NaiveTime::from_hms_opt(0, 0, 0);
    }
    if input.contains("morning") {
        return NaiveTime::from_hms_opt(9, 0, 0);
    }
    if input.contains("afternoon") {
        return NaiveTime::from_hms_opt(14, 0, 0);
    }
    if input.contains("evening") && !input.contains("this evening") {
        return NaiveTime::from_hms_opt(18, 0, 0);
    }

    // "at HH:MM" or "at H" patterns (with optional am/pm)
    let at_pos = input.find("at ").map(|p| p + 3);
    // Also try bare "Xam" / "Xpm" without "at"
    let time_str = if let Some(pos) = at_pos {
        Some(&input[pos..])
    } else {
        // Look for patterns like "3pm", "10am", "2:30pm" anywhere
        find_bare_time(input)
    };

    if let Some(s) = time_str {
        return parse_time_str(s);
    }

    None
}

/// Find a bare time like "3pm" or "10:30am" in the input.
fn find_bare_time(input: &str) -> Option<&str> {
    for (i, _) in input.match_indices(|c: char| c.is_ascii_digit()) {
        let rest = &input[i..];
        // Check if this digit sequence is followed by am/pm
        let num_end = rest
            .find(|c: char| !c.is_ascii_digit() && c != ':')
            .unwrap_or(rest.len());
        if num_end < rest.len() {
            let suffix = &rest[num_end..];
            if suffix.starts_with("am")
                || suffix.starts_with("pm")
                || suffix.starts_with("a.m")
                || suffix.starts_with("p.m")
            {
                return Some(rest);
            }
        }
    }
    None
}

/// Parse a time string like "10", "10am", "3:30pm", "15:00".
fn parse_time_str(s: &str) -> Option<NaiveTime> {
    let s = s.trim();

    // Extract digits and optional colon
    let mut hour_s = String::new();
    let mut min_s = String::new();
    let mut past_colon = false;
    let mut rest_start = 0;

    for (i, ch) in s.char_indices() {
        if ch.is_ascii_digit() {
            if past_colon { &mut min_s } else { &mut hour_s }.push(ch);
        } else if ch == ':' {
            past_colon = true;
        } else {
            rest_start = i;
            break;
        }
        rest_start = i + 1;
    }

    let mut hour: u32 = hour_s.parse().ok()?;
    let min: u32 = min_s.parse().unwrap_or(0);

    // am/pm
    let suffix = s[rest_start..].trim().to_lowercase();
    if suffix.starts_with("pm") || suffix.starts_with("p.m") {
        if hour < 12 {
            hour += 12;
        }
    } else if suffix.starts_with("am") || suffix.starts_with("a.m") {
        if hour == 12 {
            hour = 0;
        }
    }

    NaiveTime::from_hms_opt(hour, min, 0)
}

/// Parse absolute date references from input text.
///
/// Handles:
/// - Chinese: "5月18日", "12月3号"
/// - English months: "May 18", "May 18th", "December 3rd"
/// - Slash formats: "5/18", "05/18", "5/18/2026"
/// - Dash formats: "5-18", "2026-05-18"
/// - Chinese relative: "后天", "明天", "今天", "大后天"
fn parse_absolute_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let year = today.year();

    // Chinese relative days
    if input.contains("大后天") {
        return Some(today + Duration::days(3));
    }
    if input.contains("后天") {
        return Some(today + Duration::days(2));
    }
    if input.contains("明天") {
        return Some(today + Duration::days(1));
    }
    if input.contains("今天") || input.contains("今晚") {
        return Some(today);
    }

    // Chinese: "X月Y日" or "X月Y号"
    if let Some(date) = parse_chinese_date(input, year) {
        return Some(date);
    }

    let lower = input.to_lowercase();

    // English month names: "May 18", "January 3rd"
    let months = [
        ("january", 1),
        ("february", 2),
        ("march", 3),
        ("april", 4),
        ("may", 5),
        ("june", 6),
        ("july", 7),
        ("august", 8),
        ("september", 9),
        ("october", 10),
        ("november", 11),
        ("december", 12),
        ("jan", 1),
        ("feb", 2),
        ("mar", 3),
        ("apr", 4),
        ("jun", 6),
        ("jul", 7),
        ("aug", 8),
        ("sep", 9),
        ("oct", 10),
        ("nov", 11),
        ("dec", 12),
    ];
    for (name, month) in &months {
        if let Some(pos) = lower.find(name) {
            let after = lower[pos + name.len()..].trim_start();
            let day_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(day) = day_str.parse::<u32>() {
                if let Some(d) = NaiveDate::from_ymd_opt(year, *month, day) {
                    // If the date is in the past, assume next year
                    return Some(if d < today {
                        NaiveDate::from_ymd_opt(year + 1, *month, day).unwrap_or(d)
                    } else {
                        d
                    });
                }
            }
        }
    }

    // Slash: "5/18", "05/18", "5/18/2026"
    if let Some(date) = parse_slash_date(&lower, today) {
        return Some(date);
    }

    // ISO-ish: "2026-05-18"
    if let Ok(d) = NaiveDate::parse_from_str(&lower.trim(), "%Y-%m-%d") {
        return Some(d);
    }

    None
}

/// Parse Chinese date: "5月18日", "12月3号", "5月18"
fn parse_chinese_date(input: &str, year: i32) -> Option<NaiveDate> {
    let month_pos = input.find('月')?;
    let before_month = &input[..month_pos];
    let month_str: String = before_month
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let month: u32 = month_str.parse().ok()?;

    let after_month = &input[month_pos + '月'.len_utf8()..];
    let day_str: String = after_month
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let day: u32 = day_str.parse().ok()?;

    NaiveDate::from_ymd_opt(year, month, day)
}

/// Parse slash-separated dates: "5/18", "05/18", "5/18/2026"
fn parse_slash_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    // Find M/D or M/D/Y pattern in the input
    for (i, _) in input.match_indices('/') {
        // Look backwards for month digits
        let before = &input[..i];
        let month_str: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if month_str.is_empty() {
            continue;
        }

        // Look forwards for day digits
        let after = &input[i + 1..];
        let day_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if day_str.is_empty() {
            continue;
        }

        let month: u32 = month_str.parse().ok()?;
        let day: u32 = day_str.parse().ok()?;

        // Check for /YYYY after day
        let after_day = &after[day_str.len()..];
        let year = if after_day.starts_with('/') {
            let year_str: String = after_day[1..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            year_str.parse::<i32>().unwrap_or(today.year())
        } else {
            today.year()
        };

        if let Some(d) = NaiveDate::from_ymd_opt(year, month, day) {
            return Some(d);
        }
    }
    None
}

fn tenant_id_from_headers(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Uuid::nil())
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListRemindersQuery {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub label_id: Option<Uuid>,
    pub search: Option<String>,
    pub session_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/reminders",
    tag = "reminders",
    params(ListRemindersQuery),
    responses(
        (status = 200, description = "List of reminders matching the filter", body = Vec<Reminder>),
    ),
)]
pub async fn list_reminders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListRemindersQuery>,
) -> Result<Json<Vec<Reminder>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers);
    let filter = ReminderFilter {
        status: q.status,
        priority: q.priority,
        label_id: q.label_id,
        search: q.search,
        session_id: q.session_id,
        limit: q.limit.unwrap_or(50).min(200),
        offset: q.offset.unwrap_or(0),
    };
    let reminders = reminder_repo::list_reminders(&state.pool, tenant_id, &filter)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(reminders))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateReminderRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub due_time: Option<chrono::DateTime<chrono::Utc>>,
    pub labels: Option<Vec<Uuid>>,
    pub session_id: Option<Uuid>,
    /// Free-form context payload — used by the chat composer to stash
    /// attachment metadata (e.g. JSON-encoded image data URLs).
    pub context: Option<String>,
}

#[utoipa::path(
    post,
    path = "/reminders",
    tag = "reminders",
    request_body = CreateReminderRequest,
    responses(
        (status = 201, description = "Reminder created", body = Reminder),
        (status = 400, description = "Missing title or description", body = AppApiError),
    ),
)]
pub async fn create_reminder(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateReminderRequest>,
) -> Result<(StatusCode, Json<Reminder>), AppApiError> {
    if req.title.is_none() && req.description.is_none() {
        return Err(AppApiError::Internal(
            "title or description is required".into(),
        ));
    }
    let tenant_id = tenant_id_from_headers(&headers);
    let label_ids = req.labels.as_deref().unwrap_or(&[]);
    let new_reminder = NewReminder {
        session_id: req.session_id,
        tenant_id,
        speaker_id: None,
        assigned_to: None,
        title: req.title,
        description: req.description.unwrap_or_default(),
        priority: req.priority,
        due_time: req.due_time,
        transcript_excerpt: None,
        context: req.context,
        source_time: None,
        status: None,
        source_window_id: None,
    };
    let reminder = reminder_repo::create_reminder(&state.pool, &new_reminder, label_ids)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(reminder)))
}

#[utoipa::path(
    get,
    path = "/reminders/{id}",
    tag = "reminders",
    params(("id" = Uuid, Path, description = "Reminder ID")),
    responses(
        (status = 200, description = "Reminder", body = Reminder),
        (status = 404, description = "Reminder not found", body = AppApiError),
    ),
)]
pub async fn get_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Reminder>, AppApiError> {
    match reminder_repo::get_reminder(&state.pool, id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?
    {
        Some(r) => Ok(Json(r)),
        None => Err(AppApiError::Internal("not found".into())),
    }
}

#[utoipa::path(
    patch,
    path = "/reminders/{id}",
    tag = "reminders",
    params(("id" = Uuid, Path, description = "Reminder ID")),
    request_body = PatchReminderRequest,
    responses(
        (status = 200, description = "Updated reminder", body = Reminder),
        (status = 400, description = "Invalid status or priority value", body = AppApiError),
        (status = 404, description = "Reminder not found", body = AppApiError),
    ),
)]
pub async fn patch_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<PatchReminderRequest>,
) -> Result<Json<Reminder>, AppApiError> {
    if let Some(ref s) = patch.status {
        // 'pending' admits user-confirm (→open) or dismiss (→archived) of
        // medium-confidence auto-extracted items from the window extractor.
        if !["open", "pending", "completed", "archived"].contains(&s.as_str()) {
            return Err(AppApiError::Internal(format!("invalid status: {s}")));
        }
    }
    if let Some(ref p) = patch.priority {
        if !["high", "medium", "low"].contains(&p.as_str()) {
            return Err(AppApiError::Internal(format!("invalid priority: {p}")));
        }
    }
    match reminder_repo::patch_reminder(&state.pool, id, &patch)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?
    {
        Some(r) => Ok(Json(r)),
        None => Err(AppApiError::Internal("not found".into())),
    }
}

#[utoipa::path(
    delete,
    path = "/reminders/{id}",
    tag = "reminders",
    params(("id" = Uuid, Path, description = "Reminder ID")),
    responses(
        (status = 204, description = "Reminder deleted"),
        (status = 404, description = "Reminder not found", body = AppApiError),
    ),
)]
pub async fn delete_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = reminder_repo::delete_reminder(&state.pool, id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError::Internal("not found".into()))
    }
}

/// `GET /reminders/{id}/trace` — returns the reminder's provenance: which
/// extraction window produced it, the time bounds of that window, the
/// session's wall-clock start, and every finalized transcript line whose
/// [start,end] overlaps the window, joined to speaker names.
///
/// Reminders created outside the windowed extractor (manual POSTs, legacy
/// session-end generator) have no `source_window_id`; in that case we still
/// return the envelope with `window_*` fields null and `lines` empty so the
/// frontend can render a "no context available" state without a separate
/// error path.
#[utoipa::path(
    get,
    path = "/reminders/{id}/trace",
    tag = "reminders",
    params(("id" = Uuid, Path, description = "Reminder ID")),
    responses(
        (status = 200, description = "Reminder provenance", body = ReminderTrace),
        (status = 404, description = "Reminder not found", body = AppApiError),
    ),
)]
pub async fn get_reminder_trace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ReminderTrace>, AppApiError> {
    let reminder = reminder_repo::get_reminder(&state.pool, id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?
        .ok_or_else(|| AppApiError::Internal("not found".into()))?;

    let window_id: Option<Uuid> = reminder
        .source_window_id
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok());

    let session_id: Option<Uuid> = reminder
        .session_id
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok());

    let session_started_at: Option<chrono::DateTime<chrono::Utc>> = match session_id {
        Some(sid) => sqlx::query_as::<_, (chrono::DateTime<chrono::Utc>,)>(
            "SELECT started_at FROM audio_sessions WHERE id = ?1",
        )
        .bind(sid.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?
        .map(|(t,)| t),
        None => None,
    };

    // Resolve source: try audio_clips first (post-batch-clip-processing
    // rows), fall back to extraction_windows (legacy time-window scheduler
    // rows). Both populate the same source_window_id column with no FK.
    enum SourceKind {
        Clip,
        LegacyWindow,
        None,
    }
    let (source_kind, window_start_ms, window_end_ms) = match window_id {
        Some(wid) => {
            let clip: Option<(i64, i64)> = sqlx::query_as(
                "SELECT started_at_ms, ended_at_ms FROM audio_clips WHERE id = ?1",
            )
            .bind(wid.to_string())
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| AppApiError::Internal(e.to_string()))?;
            match clip {
                Some((s, e)) => (SourceKind::Clip, Some(s), Some(e)),
                None => {
                    let legacy: Option<(i64, i64)> = sqlx::query_as(
                        "SELECT start_ms, end_ms FROM extraction_windows WHERE id = ?1",
                    )
                    .bind(wid.to_string())
                    .fetch_optional(&state.pool)
                    .await
                    .map_err(|e| AppApiError::Internal(e.to_string()))?;
                    match legacy {
                        Some((s, e)) => (SourceKind::LegacyWindow, Some(s), Some(e)),
                        None => (SourceKind::None, None, None),
                    }
                }
            }
        }
        None => (SourceKind::None, None, None),
    };

    // Fetch transcripts. For clip sources, use the audio_segments.clip_id
    // linkage (new and exact). For legacy windows, fall back to the
    // overlap-by-start/end time query the old code path used.
    let lines = match (&source_kind, session_id, window_start_ms, window_end_ms) {
        (SourceKind::Clip, _, _, _) => {
            let wid = window_id.expect("Clip kind implies window_id present");
            sqlx::query_as::<_, (i64, i64, String, Option<String>, Option<String>, String, Option<String>)>(
                r#"SELECT t.start_ms, t.end_ms, t.text, sp.id, sp.display_name,
                          s.id, s.clip_id
                   FROM transcripts t
                   JOIN audio_segments s ON s.id = t.segment_id
                   LEFT JOIN speakers sp ON sp.id = s.speaker_id
                   WHERE s.clip_id = ?1
                     AND t.is_final = 1
                   ORDER BY t.start_ms"#,
            )
            .bind(wid.to_string())
            .fetch_all(&state.pool)
            .await
            .map_err(|e| AppApiError::Internal(e.to_string()))?
            .into_iter()
            .map(
                |(s_ms, e_ms, text, sid, name, seg_id, clip_id_opt)| ReminderTraceLine {
                    start_ms: s_ms,
                    end_ms: e_ms,
                    text,
                    speaker_id: sid,
                    speaker_name: name,
                    clip_id: clip_id_opt,
                    segment_id: Some(seg_id),
                },
            )
            .collect()
        }
        (SourceKind::LegacyWindow, Some(sid), Some(start), Some(end)) => {
            sqlx::query_as::<_, (i64, i64, String, Option<String>, Option<String>)>(
                r#"SELECT t.start_ms, t.end_ms, t.text, sp.id, sp.display_name
                   FROM transcripts t
                   LEFT JOIN audio_segments s ON s.id = t.segment_id
                   LEFT JOIN speakers sp      ON sp.id = s.speaker_id
                   WHERE t.session_id = ?1
                     AND t.is_final = 1
                     AND t.start_ms < ?3
                     AND t.end_ms   > ?2
                   ORDER BY t.start_ms"#,
            )
            .bind(sid.to_string())
            .bind(start)
            .bind(end)
            .fetch_all(&state.pool)
            .await
            .map_err(|e| AppApiError::Internal(e.to_string()))?
            .into_iter()
            .map(|(s_ms, e_ms, text, sid, name)| ReminderTraceLine {
                start_ms: s_ms,
                end_ms: e_ms,
                text,
                speaker_id: sid,
                speaker_name: name,
                clip_id: None,
                segment_id: None,
            })
            .collect()
        }
        _ => Vec::new(),
    };

    Ok(Json(ReminderTrace {
        reminder_id: reminder.id,
        session_id: reminder.session_id,
        window_id: reminder.source_window_id,
        window_start_ms,
        window_end_ms,
        session_started_at,
        transcript_excerpt: reminder.transcript_excerpt,
        source_time: reminder.source_time,
        lines,
    }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ExtractRemindersRequest {
    pub text: String,
    #[serde(default)]
    pub images: Vec<ImageInput>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ImageInput {
    /// Full data URL, e.g. "data:image/png;base64,...."
    pub data_url: String,
}

#[utoipa::path(
    post,
    path = "/reminders/extract",
    tag = "reminders",
    request_body = ExtractRemindersRequest,
    responses(
        (status = 200, description = "Extracted reminders (may be empty)", body = Vec<Reminder>),
        (status = 400, description = "Empty text and no images", body = AppApiError),
        (status = 503, description = "LLM router disabled", body = AppApiError),
    ),
)]
pub async fn extract_reminders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ExtractRemindersRequest>,
) -> Result<Json<Vec<Reminder>>, AppApiError> {
    let text = req.text.trim().to_string();
    if text.is_empty() && req.images.is_empty() {
        return Err(AppApiError::Internal(
            "text or at least one image is required".into(),
        ));
    }

    let tenant_id = tenant_id_from_headers(&headers);

    let router = state.router.read().await;
    if router.is_disabled() {
        return Err(AppApiError::Internal("no LLM backend is configured".into()));
    }

    // Fetch available labels so the model can tag items.
    let db_labels = label_repo::list_labels(&state.pool, tenant_id)
        .await
        .unwrap_or_default();
    let label_names: Vec<String> = db_labels.iter().map(|l| l.name.clone()).collect();

    let image_urls: Vec<String> = req.images.into_iter().map(|i| i.data_url).collect();
    let todo_items = router
        .generate_todos(&text, &label_names, &image_urls)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "extract_reminders: LLM extraction failed");
            AppApiError::Internal(format!("LLM extraction failed: {e}"))
        })?;

    let mut created_reminders = Vec::new();

    for item in todo_items {
        // Use model's title if provided, otherwise truncate description
        let title_text = item.title.unwrap_or_else(|| {
            if item.description.len() > 60 {
                format!("{}...", &item.description[..57])
            } else {
                item.description.clone()
            }
        });

        // Resolve due_time: prefer our own relative-date parser over the
        // model's output (small models can't do date math reliably).
        let resolved_naive = resolve_relative_datetime(&text);
        tracing::info!(
            input = %text,
            resolver_result = ?resolved_naive,
            model_due_time = ?item.due_time,
            "due_time resolution"
        );
        let resolved_naive = resolved_naive.or_else(|| {
            // Fallback to model's output if we didn't detect a reference
            item.due_time.as_deref().and_then(|s| {
                NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
                    .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
                    .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M"))
                    .ok()
            })
        });
        let due_time = resolved_naive.and_then(|naive| {
            chrono::Local
                .from_local_datetime(&naive)
                .single()
                .map(|local| local.with_timezone(&chrono::Utc))
        });

        // Resolve label names → UUIDs by case-insensitive match.
        let label_ids: Vec<Uuid> = item
            .labels
            .iter()
            .filter_map(|name| {
                db_labels
                    .iter()
                    .find(|l| l.name.eq_ignore_ascii_case(name))
                    .and_then(|l| l.id.parse().ok())
            })
            .collect();

        let new_reminder = NewReminder {
            session_id: None,
            tenant_id,
            speaker_id: None,
            assigned_to: None,
            title: Some(title_text),
            description: item.description,
            priority: item.priority.clone(),
            due_time,
            transcript_excerpt: None,
            context: None,
            source_time: None,
            status: None,
            source_window_id: None,
        };

        match reminder_repo::create_reminder(&state.pool, &new_reminder, &label_ids).await {
            Ok(reminder) => created_reminders.push(reminder),
            Err(e) => {
                tracing::warn!(error = %e, "extract_reminders: failed to persist one todo item");
            }
        }
    }

    Ok(Json(created_reminders))
}
