//! The terminal operator: renders events and answers approval and clarification requests.

use std::io::{self, Write};

use igor_core::CancellationToken;
use igor_core::approval::Decision;
use igor_core::events::{EventEnvelope, RuntimeEvent};
use igor_core::session::SessionHandle;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use tokio::sync::mpsc;

/// Renders events until the turn finishes, prompting on `input` for approvals and
/// clarifications. Each event is also written to `export`, redacted, when given.
///
/// Only an explicit `y` or `yes` approves. Any other answer, or end of input, denies. End of
/// input at a clarification prompt cancels the turn.
pub async fn operate<R, W>(
    mut events: mpsc::UnboundedReceiver<EventEnvelope>,
    handle: &SessionHandle,
    input: &mut R,
    out: &mut W,
    mut export: Option<&mut dyn Write>,
    cancel: &CancellationToken,
) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: Write,
{
    while let Some(envelope) = events.recv().await {
        crate::render::render(&envelope.event, out)?;
        if let Some(export) = export.as_deref_mut() {
            crate::export::write_event(&envelope, export)?;
        }
        match envelope.event {
            RuntimeEvent::ApprovalRequested { request } => {
                write!(out, "Approve this action? [y/N] ")?;
                out.flush()?;
                let answer = read_line(input).await?;
                let decision = match answer.as_deref().map(str::trim) {
                    Some(text)
                        if text.eq_ignore_ascii_case("y") || text.eq_ignore_ascii_case("yes") =>
                    {
                        Decision::Approve
                    }
                    _ => Decision::Deny,
                };
                if handle
                    .resolve_approval(request.approval_id, request.action_hash, decision)
                    .is_err()
                {
                    return Ok(());
                }
            }
            RuntimeEvent::ClarificationRequested { request_id, .. } => {
                write!(out, "Your answer: ")?;
                out.flush()?;
                match read_line(input).await? {
                    Some(answer) => {
                        if handle
                            .answer_clarification(request_id, answer.trim())
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                    None => cancel.cancel(),
                }
            }
            RuntimeEvent::TurnFinished { .. } => break,
            _ => {}
        }
    }
    if let Some(export) = export {
        export.flush()?;
    }
    out.flush()
}

/// Reads one line; `None` at end of input.
pub async fn read_line<R: AsyncBufRead + Unpin>(input: &mut R) -> io::Result<Option<String>> {
    let mut line = String::new();
    let read = input.read_line(&mut line).await?;
    Ok((read > 0).then_some(line))
}
