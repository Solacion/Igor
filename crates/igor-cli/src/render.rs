//! Plain-text rendering of runtime events.
//!
//! Model output, tool names and tool results are untrusted. Control characters other than
//! newline and tab are escaped, so they cannot drive the terminal (for example with ANSI escape
//! sequences).

use std::io::{self, Write};

use igor_core::approval::ApprovalRequest;
use igor_core::events::{ApprovalOutcome, CallFailure, LimitKind, RuntimeEvent, TurnOutcome};

/// Escapes control characters except newline and tab.
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() && c != '\n' && c != '\t' {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

/// Writes one event as plain text. Events that need no output write nothing.
pub fn render(event: &RuntimeEvent, out: &mut impl Write) -> io::Result<()> {
    match event {
        RuntimeEvent::ConnectionChanged {
            server,
            state,
            detail,
        } => {
            let detail = detail.as_deref().map(sanitize).unwrap_or_default();
            writeln!(out, "[server {server}: {state:?}] {detail}")
        }
        RuntimeEvent::ToolCallProposed {
            call_id,
            tool,
            arguments,
        } => writeln!(
            out,
            "[{call_id}] call {} {}",
            sanitize(&tool.to_string()),
            sanitize(&arguments.to_string())
        ),
        RuntimeEvent::ToolCallRejected {
            call_id,
            alias,
            reason,
            ..
        } => writeln!(
            out,
            "[{call_id}] refused {}: {}",
            sanitize(alias),
            sanitize(reason)
        ),
        RuntimeEvent::ToolCallStarted { .. } => Ok(()),
        RuntimeEvent::ToolCallCompleted {
            call_id,
            is_error,
            truncated,
        } => {
            let status = if *is_error {
                "returned an error"
            } else {
                "done"
            };
            let note = if *truncated {
                " (result truncated)"
            } else {
                ""
            };
            writeln!(out, "[{call_id}] {status}{note}")
        }
        RuntimeEvent::ToolCallFailed {
            call_id,
            outcome,
            message,
        } => {
            let outcome = match outcome {
                CallFailure::NotExecuted => "not executed",
                CallFailure::Unknown => "OUTCOME UNKNOWN, it may have run",
            };
            writeln!(out, "[{call_id}] failed, {outcome}: {}", sanitize(message))
        }
        RuntimeEvent::AssistantText { text } if text.trim().is_empty() => Ok(()),
        RuntimeEvent::AssistantText { text } => writeln!(out, "\n{}\n", sanitize(text.trim())),
        RuntimeEvent::ApprovalRequested { request } => render_approval(request, out),
        RuntimeEvent::ApprovalResolved {
            approval_id,
            outcome,
            ..
        } => {
            let outcome = match outcome {
                ApprovalOutcome::Approved => "approved",
                ApprovalOutcome::Denied => "denied, not executed",
                ApprovalOutcome::Cancelled => "cancelled, not executed",
                ApprovalOutcome::Invalidated => "invalidated, not executed",
            };
            writeln!(out, "[{approval_id}] {outcome}")
        }
        RuntimeEvent::ClarificationRequested { question, .. } => {
            writeln!(out, "\nIGOR asks: {}", sanitize(question))
        }
        RuntimeEvent::ClarificationAnswered { .. } => Ok(()),
        RuntimeEvent::InputRejected { id, reason } => {
            writeln!(
                out,
                "[{}] input refused: {}",
                sanitize(id),
                sanitize(reason)
            )
        }
        RuntimeEvent::TurnFinished { outcome } => writeln!(out, "[{}]", describe(outcome)),
        _ => Ok(()),
    }
}

fn render_approval(request: &ApprovalRequest, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "\nApproval required [{}]", request.approval_id)?;
    writeln!(
        out,
        "  tool:        {}",
        sanitize(&request.tool.to_string())
    )?;
    writeln!(
        out,
        "  arguments:   {}",
        sanitize(&request.canonical_arguments)
    )?;
    writeln!(out, "  action hash: {}", request.action_hash)
}

/// Short description of a turn outcome.
pub fn describe(outcome: &TurnOutcome) -> String {
    match outcome {
        TurnOutcome::Completed => "completed".to_owned(),
        TurnOutcome::LimitReached { limit } => {
            let limit = match limit {
                LimitKind::ModelRequests => "model requests",
                LimitKind::ToolCalls => "tool calls",
                LimitKind::OutputTokens => "output tokens",
            };
            format!("stopped: {limit} limit reached; the answer is incomplete")
        }
        TurnOutcome::Cancelled => "cancelled".to_owned(),
        TurnOutcome::Failed { message } => format!("failed: {}", sanitize(message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_assistant_text_prints_nothing() -> io::Result<()> {
        let mut out = Vec::new();
        render(
            &RuntimeEvent::AssistantText {
                text: "\n \n".to_owned(),
            },
            &mut out,
        )?;
        assert!(out.is_empty());
        render(
            &RuntimeEvent::AssistantText {
                text: "\nDone.\n".to_owned(),
            },
            &mut out,
        )?;
        assert_eq!(String::from_utf8_lossy(&out), "\nDone.\n\n");
        Ok(())
    }

    #[test]
    fn sanitize_escapes_terminal_control_sequences() {
        assert_eq!(sanitize("ok\n\tline"), "ok\n\tline");
        assert_eq!(sanitize("\u{1b}[31mred"), "\\u{1b}[31mred");
        assert_eq!(sanitize("bell\u{7}"), "bell\\u{7}");
    }

    #[test]
    fn renders_text_without_escape_sequences() -> io::Result<()> {
        let mut out = Vec::new();
        render(
            &RuntimeEvent::AssistantText {
                text: "\u{1b}]0;title\u{7}hi".to_owned(),
            },
            &mut out,
        )?;
        let text = String::from_utf8_lossy(&out);
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("hi"));
        Ok(())
    }
}
