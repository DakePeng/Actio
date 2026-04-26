use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub const SYSTEM_PROMPT: &str = "\
You are a task extraction assistant. Convert user input into a structured task.\n\
\n\
Output ONLY a single JSON object — no markdown, no fences, no explanation:\n\
{\"title\": \"...\", \"description\": \"...\", \"priority\": \"high|medium|low\", \"due_time\": \"YYYY-MM-DDTHH:MM\", \"labels\": [\"...\"]}\n\
\n\
Fields:\n\
- title: short task name, under 50 chars. Use the same language as the input.\n\
- description: full details — who, what, where, when, why. Expand abbreviations. Same language as input.\n\
- priority: \"high\" (urgent/deadline soon), \"medium\" (normal), \"low\" (whenever). Default to \"medium\".\n\
- due_time: local time as \"YYYY-MM-DDTHH:MM\". Resolve \"tomorrow\" from today's date. Omit if no time reference.\n\
- labels: pick 0-3 from the available list. Empty array if none fit.\n\
\n\
Keep it simple. One JSON object. No extra text.";

pub fn build_todo_messages(transcript: &str, label_names: &[String]) -> Vec<ChatMessage> {
    let today = chrono::Local::now().format("%Y-%m-%d %A").to_string();
    let labels_str = if label_names.is_empty() {
        "none".to_string()
    } else {
        label_names.join(", ")
    };
    let system = format!("Today: {today}\nLabels: [{labels_str}]\n\n{SYSTEM_PROMPT}");
    vec![
        ChatMessage {
            role: "system".into(),
            content: system,
        },
        ChatMessage {
            role: "user".into(),
            content: transcript.to_string(),
        },
    ]
}

/// Prompt for the windowed extractor. Transcript lines arrive pre-formatted
/// as `[HH:MM:SS • Speaker]: text` so the LLM can quote back a verbatim
/// `evidence_quote` and name the `speaker_name` it came from.
///
/// The model returns `{"items": [...]}` where each item includes
/// `confidence: "high"|"medium"|"low"`. Confidence drives the routing at
/// the caller: high → open, medium → pending (review queue), low → dropped.
pub const WINDOW_SYSTEM_PROMPT: &str = "\
You are listening to a rolling window of conversation and extracting only the CERTAIN action items.\n\
Be conservative: most idle talk is NOT an action item. Missing items is better than inventing them.\n\
\n\
Return ONLY a raw JSON object — no markdown, no fences, no explanation:\n\
{\"items\":[{\"title\":\"...\",\"description\":\"...\",\"priority\":\"high|medium|low\",\"due_time\":\"YYYY-MM-DDTHH:MM\",\"labels\":[\"...\"],\"confidence\":\"high|medium|low\",\"evidence_quote\":\"verbatim span from input\",\"speaker_name\":\"name as printed, or null\"}]}\n\
\n\
Rules:\n\
- If nothing in this window is a real action item, return {\"items\":[]}.\n\
- confidence=\"high\": explicit commitment or ask, unambiguous. Example: \\\"Remind me to email Bob tomorrow at 9.\\\"\n\
- confidence=\"medium\": plausibly an action but phrasing is ambiguous (\\\"maybe we should …\\\", \\\"someone could …\\\"). Use sparingly.\n\
- confidence=\"low\": do NOT return these — omit them entirely.\n\
- evidence_quote MUST be a verbatim substring from the input, trimmed. If you can't pick one, the item is not real — omit it.\n\
- speaker_name is copied from the bracketed speaker tag in the input line containing the evidence_quote, or null if Unknown.\n\
- title under 60 chars, same language as input. description expands context naturally. due_time only if an explicit time reference exists in this window.\n\
- labels: pick 0–3 from the provided list. Empty array if none fit.";

/// Profiled variant — used when a `TenantProfile` is available. Adds an
/// ownership rule (item must belong to the user) and a concreteness rule
/// (verb-object plus deadline / recipient / urgency). The `{display_name}`,
/// `{aliases_line}`, and `{bio_block}` placeholders are filled by
/// `build_window_messages`.
pub const WINDOW_SYSTEM_PROMPT_PROFILED_TEMPLATE: &str = "\
You are extracting action items FOR {display_name}.\n\
{aliases_line}\
{bio_block}\
\n\
Their voice is tagged in the transcript as \"{display_name}\".\n\
Other speakers are other people — friends, coworkers, voices on a podcast, LLM TTS, anyone.\n\
\n\
Extract an item ONLY when BOTH of these are true:\n\
\n\
(1) OWNERSHIP — the item belongs to {display_name}. Qualifies if any of:\n\
    a. {display_name} commits (\"I'll send the doc\", \"let me check on that\").\n\
    b. {display_name} is asked or assigned by name or by direct address (\"Hey Dake, can you…\", \"@DK could you…\", \"你能不能…\").\n\
    c. another speaker promises a deliverable TO {display_name} (\"I'll send YOU the API spec by Friday\").\n\
\n\
(2) CONCRETENESS — at least one of:\n\
    a. explicit time (\"by Friday 3pm\", \"tomorrow morning\", \"EOD\").\n\
    b. named recipient or counterparty (\"to Bob\", \"with the design team\").\n\
    c. urgency keyword (\"ASAP\", \"today\", \"now\", \"before the demo\").\n\
\n\
If unsure who owns an item, drop it. If it's vague aspiration (\"I should look into that someday\", \"we ought to\"), drop it.\n\
\n\
Return ONLY a raw JSON object — no markdown, no fences, no explanation:\n\
{\"items\":[{\"title\":\"...\",\"description\":\"...\",\"priority\":\"high|medium|low\",\"due_time\":\"YYYY-MM-DDTHH:MM\",\"labels\":[\"...\"],\"confidence\":\"high|medium\",\"evidence_quote\":\"verbatim substring from input\",\"speaker_name\":\"name as printed, or null\"}]}\n\
\n\
confidence=\"high\": both legs unambiguous.\n\
confidence=\"medium\": both legs satisfied but phrasing leaves real doubt.\n\
Do not emit \"low\" — omit the item instead.\n\
evidence_quote MUST be a verbatim substring. title under 60 chars, same language as input. labels: pick 0–3 from the provided list.";

pub fn build_window_messages(
    attributed_transcript: &str,
    label_names: &[String],
    window_local_date: &str,
    profile: Option<&crate::domain::types::TenantProfile>,
) -> Vec<ChatMessage> {
    let labels_str = if label_names.is_empty() {
        "none".to_string()
    } else {
        label_names.join(", ")
    };

    let body = match profile {
        None => WINDOW_SYSTEM_PROMPT.to_string(),
        Some(p) => render_profiled_prompt(p),
    };

    let system = format!(
        "Window date (local): {window_local_date}\nLabels: [{labels_str}]\n\n{body}"
    );
    vec![
        ChatMessage { role: "system".into(), content: system },
        ChatMessage { role: "user".into(),   content: attributed_transcript.to_string() },
    ]
}

fn render_profiled_prompt(profile: &crate::domain::types::TenantProfile) -> String {
    let display = profile.display_name.as_deref().unwrap_or("the user");
    let aliases_line = if profile.aliases.is_empty() {
        String::new()
    } else {
        format!("They may also be addressed as: {}.\n", profile.aliases.join(", "))
    };
    let bio_block = match profile.bio.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(b) => format!("About them:\n{b}\n"),
        None => String::new(),
    };
    WINDOW_SYSTEM_PROMPT_PROFILED_TEMPLATE
        .replace("{display_name}", display)
        .replace("{aliases_line}", &aliases_line)
        .replace("{bio_block}", &bio_block)
}

/// Build a retry prompt that includes the failed output so the model can self-correct.
pub fn build_retry_messages(
    transcript: &str,
    label_names: &[String],
    failed_json: &str,
) -> Vec<ChatMessage> {
    let mut msgs = build_todo_messages(transcript, label_names);
    msgs.push(ChatMessage {
        role: "assistant".into(),
        content: failed_json.to_string(),
    });
    msgs.push(ChatMessage {
        role: "user".into(),
        content: "Invalid. Return ONLY a raw JSON object. No markdown. No code fences. Fix it."
            .to_string(),
    });
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_todo_messages_has_system_then_user() {
        let labels = vec!["Work".into(), "Personal".into()];
        let msgs = build_todo_messages("Alice: do the thing", &labels);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("Alice: do the thing"));
        assert!(msgs[0].content.contains("Work, Personal"));
    }

    #[test]
    fn system_prompt_demands_json() {
        assert!(SYSTEM_PROMPT.contains("ONLY a single JSON"));
    }

    #[test]
    fn empty_labels_shows_none() {
        let msgs = build_todo_messages("test", &[]);
        assert!(msgs[0].content.contains("[none]"));
    }

    use crate::domain::types::TenantProfile;
    use uuid::Uuid;

    fn fixture_profile() -> TenantProfile {
        TenantProfile {
            tenant_id: Uuid::new_v4(),
            display_name: Some("Dake Peng".into()),
            aliases: vec!["Dake".into(), "DK".into(), "彭大可".into()],
            bio: Some("Solo dev building Actio.".into()),
        }
    }

    #[test]
    fn build_window_messages_no_profile_matches_legacy_byte_for_byte() {
        let labels = vec!["Work".into()];
        let with_none = build_window_messages("hi", &labels, "2026-04-26 Sunday", None);
        let sys = &with_none[0].content;
        assert!(sys.contains(WINDOW_SYSTEM_PROMPT));
        assert!(!sys.contains("Extracting action items FOR"));
        assert!(!sys.contains("They may also be addressed as"));
    }

    #[test]
    fn build_window_messages_with_profile_includes_all_fields() {
        let labels: Vec<String> = vec![];
        let profile = fixture_profile();
        let msgs = build_window_messages("hello", &labels, "2026-04-26 Sunday", Some(&profile));
        let sys = &msgs[0].content;
        assert!(sys.contains("Dake Peng"), "missing display_name");
        assert!(sys.contains("Dake"), "missing alias 1");
        assert!(sys.contains("DK"), "missing alias 2");
        assert!(sys.contains("彭大可"), "missing CJK alias");
        assert!(sys.contains("Solo dev building Actio."), "missing bio");
        assert!(sys.contains("OWNERSHIP"), "missing ownership rule");
        assert!(sys.contains("CONCRETENESS"), "missing concreteness rule");
    }

    #[test]
    fn build_window_messages_with_profile_omits_about_when_bio_blank() {
        let mut p = fixture_profile();
        p.bio = Some("   ".into());
        let msgs = build_window_messages("hi", &[], "2026-04-26 Sunday", Some(&p));
        let sys = &msgs[0].content;
        assert!(!sys.contains("About them:"), "should not render the About them: header for blank bio");
    }

    #[test]
    fn build_window_messages_with_profile_omits_aliases_line_when_empty() {
        let mut p = fixture_profile();
        p.aliases.clear();
        let msgs = build_window_messages("hi", &[], "2026-04-26 Sunday", Some(&p));
        let sys = &msgs[0].content;
        assert!(!sys.contains("They may also be addressed as"), "no alias line when list empty");
    }
}
