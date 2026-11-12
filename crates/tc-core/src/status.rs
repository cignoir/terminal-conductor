//! Turns Claude Code hook payloads into per-pane status updates.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeStatus {
    Idle,
    Working,
    NeedsPermission,
    Done,
    Error,
    Exited,
}

/// A hook message as sent by the `hook` subcommand over the pipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookMessage {
    pub pane_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatusUpdate {
    pub pane_id: String,
    pub event: String,
    pub status: Option<ClaudeStatus>,
    pub session_id: Option<String>,
    pub session_title: Option<String>,
    pub last_assistant_message: Option<String>,
    pub notification_type: Option<String>,
    pub message: Option<String>,
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Maps one hook payload to a status update. Pure; the caller owns state.
pub fn interpret(msg: &HookMessage) -> StatusUpdate {
    let p = &msg.payload;
    let event = str_field(p, "hook_event_name").unwrap_or_default();
    let notification_type = str_field(p, "notification_type");
    // Subagent hooks carry agent_id; they must not flip the main session state.
    let is_subagent = p.get("agent_id").map(|v| !v.is_null()).unwrap_or(false);

    let status = if is_subagent {
        None
    } else {
        match event.as_str() {
            "SessionStart" => Some(ClaudeStatus::Idle),
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
            | "PreCompact" => Some(ClaudeStatus::Working),
            "PermissionRequest" => Some(ClaudeStatus::NeedsPermission),
            "Notification" => match notification_type.as_deref() {
                Some("permission_prompt") | Some("elicitation_dialog")
                | Some("agent_needs_input") => Some(ClaudeStatus::NeedsPermission),
                _ => None,
            },
            "Stop" => Some(ClaudeStatus::Done),
            "StopFailure" => Some(ClaudeStatus::Error),
            "SessionEnd" => match str_field(p, "reason").as_deref() {
                // /clear and /resume end one session and immediately start another.
                Some("clear") | Some("resume") => None,
                _ => Some(ClaudeStatus::Exited),
            },
            _ => None,
        }
    };

    let last_assistant_message = if event == "Stop" && !is_subagent {
        str_field(p, "last_assistant_message").map(|s| truncate(&s, 600))
    } else {
        None
    };

    StatusUpdate {
        pane_id: msg.pane_id.clone(),
        event,
        status,
        session_id: if is_subagent {
            None
        } else {
            str_field(p, "session_id")
        },
        session_title: str_field(p, "session_title"),
        last_assistant_message,
        notification_type,
        message: str_field(p, "message"),
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max_chars {
        t.to_string()
    } else {
        let mut out: String = t.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(payload: Value) -> HookMessage {
        HookMessage {
            pane_id: "p1".into(),
            payload,
        }
    }

    #[test]
    fn prompt_submit_is_working() {
        let u = interpret(&msg(json!({"hook_event_name":"UserPromptSubmit","session_id":"s1"})));
        assert_eq!(u.status, Some(ClaudeStatus::Working));
        assert_eq!(u.session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn permission_notification() {
        let u = interpret(&msg(json!({"hook_event_name":"Notification","notification_type":"permission_prompt","message":"Claude needs your permission"})));
        assert_eq!(u.status, Some(ClaudeStatus::NeedsPermission));
        let u = interpret(&msg(json!({"hook_event_name":"Notification","notification_type":"idle_prompt"})));
        assert_eq!(u.status, None);
    }

    #[test]
    fn stop_captures_last_message() {
        let u = interpret(&msg(json!({"hook_event_name":"Stop","last_assistant_message":"  done!  "})));
        assert_eq!(u.status, Some(ClaudeStatus::Done));
        assert_eq!(u.last_assistant_message.as_deref(), Some("done!"));
    }

    #[test]
    fn subagent_events_do_not_change_status() {
        let u = interpret(&msg(json!({"hook_event_name":"Stop","agent_id":"a1","session_id":"s9"})));
        assert_eq!(u.status, None);
        assert_eq!(u.session_id, None);
    }

    #[test]
    fn clear_does_not_exit() {
        let u = interpret(&msg(json!({"hook_event_name":"SessionEnd","reason":"clear"})));
        assert_eq!(u.status, None);
        let u = interpret(&msg(json!({"hook_event_name":"SessionEnd","reason":"prompt_input_exit"})));
        assert_eq!(u.status, Some(ClaudeStatus::Exited));
    }
}
