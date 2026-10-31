use super::*;
use igor_core::{
    events::RuntimeEvent,
    ids::SequentialIds,
    policy::Classification,
    registry::ToolRegistry,
    session::{SessionConfig, SessionParts},
    testing::{FakeReply, RecordingBackend, ScriptedProvider, ScriptedTurn},
    tools::{ServerId, ToolBackend, ToolDescriptor, ToolHints, ToolId, ToolOutput},
};
use serde_json::json;
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Harness {
    runtime: Arc<DesktopRuntime>,
    id: String,
    events: mpsc::UnboundedReceiver<EventEnvelope>,
    backend: Arc<RecordingBackend>,
    provider: Arc<ScriptedProvider>,
}
fn harness(
    turns: Vec<ScriptedTurn>,
    secrets: Vec<igor_config::Secret>,
) -> Result<Harness, Box<dyn std::error::Error>> {
    let runtime = Arc::new(DesktopRuntime::default());
    let tool = ToolId {
        server: ServerId::new("sales")?,
        name: "create_order".into(),
    };
    let registry = ToolRegistry::build(
        [ToolDescriptor {
            id: tool.clone(),
            description: None,
            input_schema: json!({"type":"object"}),
            hints: ToolHints::default(),
        }],
        &HashMap::from([(tool.clone(), Classification::Write)]),
    );
    let provider = Arc::new(ScriptedProvider::new(turns));
    let backend = Arc::new(RecordingBackend::new(tool.server.clone()).with_reply(
        "create_order",
        FakeReply::Output(ToolOutput {
            content: vec![],
            structured: None,
            is_error: false,
        }),
    ));
    let (sink, events) = mpsc::unbounded_channel();
    let parts = SessionParts {
        provider: provider.clone(),
        registry,
        backends: HashMap::from([(tool.server, backend.clone() as Arc<dyn ToolBackend>)]),
        ids: Arc::new(SequentialIds::new("test")),
        events: Arc::new(sink.clone()),
    };
    let (id, cancel) = runtime.reserve()?;
    runtime.install(
        id.clone(),
        cancel,
        Prepared {
            parts,
            config: SessionConfig::default(),
            servers: vec![],
            statuses: vec![],
            provider_id: "fake".into(),
            model: "scripted".into(),
            secrets,
            revision: 0,
        },
        Arc::new(sink),
    )?;
    Ok(Harness {
        runtime,
        id,
        events,
        backend,
        provider,
    })
}
fn message(id: &str) -> MessageRequest {
    MessageRequest {
        session_id: id.into(),
        text: "Create an order".into(),
    }
}
async fn event(
    events: &mut mpsc::UnboundedReceiver<EventEnvelope>,
) -> Result<EventEnvelope, Box<dyn std::error::Error>> {
    Ok(tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await?
        .ok_or("event channel closed")?)
}
async fn approval(
    events: &mut mpsc::UnboundedReceiver<EventEnvelope>,
) -> Result<igor_core::approval::ApprovalRequest, Box<dyn std::error::Error>> {
    loop {
        if let RuntimeEvent::ApprovalRequested { request } = event(events).await?.event {
            return Ok(request);
        }
    }
}

#[tokio::test]
async fn streams_ordered_events_and_preserves_conversation() -> TestResult {
    let mut h = harness(
        vec![ScriptedTurn::text("First"), ScriptedTurn::text("Second")],
        vec![],
    )?;
    assert_eq!(
        h.runtime.send(message(&h.id)).await?,
        TurnOutcome::Completed
    );
    assert_eq!(
        h.runtime.send(message(&h.id)).await?,
        TurnOutcome::Completed
    );
    let mut sequences = vec![];
    while let Ok(event) = h.events.try_recv() {
        sequences.push(event.sequence);
    }
    assert!(sequences.len() >= 4);
    assert!(sequences.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert!(h.provider.requests()[1].messages.len() > h.provider.requests()[0].messages.len());
    h.provider.verify()?;
    Ok(())
}

#[tokio::test]
async fn approval_is_required_and_dispatches_exactly_once() -> TestResult {
    let mut h = harness(
        vec![
            ScriptedTurn::tool_call("one", "sales__create_order", json!({"supplier":"X"})),
            ScriptedTurn::text("Done"),
        ],
        vec![],
    )?;
    let runtime = h.runtime.clone();
    let request = message(&h.id);
    let turn = tokio::spawn(async move { runtime.send(request).await });
    let pending = approval(&mut h.events).await?;
    assert!(h.backend.calls().is_empty());
    assert_eq!(h.runtime.send(message(&h.id)).await, Err(BridgeError::Busy));
    let request = || ApprovalRequest {
        session_id: h.id.clone(),
        approval_id: pending.approval_id.clone(),
        action_hash: pending.action_hash,
        decision: Decision::Approve,
    };
    h.runtime.approve(request())?;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), turn).await???,
        TurnOutcome::Completed
    );
    assert_eq!(h.backend.calls().len(), 1);
    assert_eq!(h.runtime.approve(request()), Err(BridgeError::NoTurn));
    Ok(())
}

#[tokio::test]
async fn wrong_hash_and_denial_never_dispatch() -> TestResult {
    for wrong_hash in [false, true] {
        let mut h = harness(
            vec![
                ScriptedTurn::tool_call("one", "sales__create_order", json!({})),
                ScriptedTurn::text("No order"),
            ],
            vec![],
        )?;
        let runtime = h.runtime.clone();
        let request = message(&h.id);
        let turn = tokio::spawn(async move { runtime.send(request).await });
        let pending = approval(&mut h.events).await?;
        let hash = if wrong_hash {
            serde_json::from_value(json!("00".repeat(32)))?
        } else {
            pending.action_hash
        };
        h.runtime.approve(ApprovalRequest {
            session_id: h.id.clone(),
            approval_id: pending.approval_id,
            action_hash: hash,
            decision: if wrong_hash {
                Decision::Approve
            } else {
                Decision::Deny
            },
        })?;
        if wrong_hash {
            loop {
                if matches!(
                    event(&mut h.events).await?.event,
                    RuntimeEvent::InputRejected { .. }
                ) {
                    break;
                }
            }
            assert!(h.backend.calls().is_empty());
            h.runtime.cancel(SessionRequest {
                session_id: h.id.clone(),
            })?;
        }
        tokio::time::timeout(Duration::from_secs(2), turn).await???;
        assert!(h.backend.calls().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn shutdown_cancels_pending_approval_and_rejects_stale_ids() -> TestResult {
    let mut h = harness(
        vec![ScriptedTurn::tool_call(
            "one",
            "sales__create_order",
            json!({}),
        )],
        vec![],
    )?;
    let runtime = h.runtime.clone();
    let request = message(&h.id);
    let turn = tokio::spawn(async move { runtime.send(request).await });
    approval(&mut h.events).await?;
    h.runtime.shutdown();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), turn).await???,
        TurnOutcome::Cancelled
    );
    assert_eq!(
        h.runtime.send(message(&h.id)).await,
        Err(BridgeError::NoSession)
    );
    assert!(h.backend.calls().is_empty());
    assert!(h.runtime.reserve().is_ok());
    Ok(())
}

#[tokio::test]
async fn credentials_are_blocked_before_crossing_ipc() -> TestResult {
    let mut h = harness(
        vec![ScriptedTurn::text("Leaked secret-123")],
        vec![igor_config::Secret::new("secret-123")],
    )?;
    assert_eq!(
        h.runtime.send(message(&h.id)).await,
        Err(BridgeError::SensitiveOutput)
    );
    while let Ok(event) = h.events.try_recv() {
        assert!(!serde_json::to_string(&event)?.contains("secret-123"));
    }
    assert!(contains_secret(
        &json!({"nested":[{"secret-123":"value"}]}),
        "secret-123"
    ));
    assert_eq!(
        h.runtime.send(message(&h.id)).await,
        Err(BridgeError::NoSession)
    );
    Ok(())
}

#[test]
fn requests_reject_unknown_fields_and_invalid_inputs() -> TestResult {
    assert!(
        serde_json::from_value::<MessageRequest>(
            json!({"session_id":"x", "text":"hello", "tool":"bypass"})
        )
        .is_err()
    );
    assert_eq!(
        StartRequest {
            provider_id: "".into()
        }
        .validate(),
        Err(BridgeError::InvalidRequest)
    );
    assert!(!valid_text(&"x".repeat(MAX_TEXT_BYTES + 1)));
    assert_eq!(
        serde_json::to_value(BridgeError::Provider)?,
        json!({"code":"provider"})
    );
    Ok(())
}

#[tokio::test]
async fn clarification_answer_reaches_the_model() -> TestResult {
    let mut h = harness(
        vec![
            ScriptedTurn::tool_call(
                "ask",
                "igor_ask_operator",
                json!({"question":"Which date?"}),
            ),
            ScriptedTurn::text("Tomorrow"),
        ],
        vec![],
    )?;
    let runtime = h.runtime.clone();
    let request = message(&h.id);
    let turn = tokio::spawn(async move { runtime.send(request).await });
    let request_id = loop {
        if let RuntimeEvent::ClarificationRequested { request_id, .. } =
            event(&mut h.events).await?.event
        {
            break request_id;
        }
    };
    h.runtime.clarify(ClarificationRequest {
        session_id: h.id.clone(),
        request_id,
        answer: "Tomorrow".into(),
    })?;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), turn).await???,
        TurnOutcome::Completed
    );
    assert!(serde_json::to_string(&h.provider.requests()[1])?.contains("Tomorrow"));
    Ok(())
}

#[tokio::test]
async fn dropped_turn_releases_busy_state() -> TestResult {
    let mut h = harness(
        vec![
            ScriptedTurn::tool_call("one", "sales__create_order", json!({})),
            ScriptedTurn::text("Recovered"),
        ],
        vec![],
    )?;
    let runtime = h.runtime.clone();
    let request = message(&h.id);
    let turn = tokio::spawn(async move { runtime.send(request).await });
    approval(&mut h.events).await?;
    turn.abort();
    assert!(turn.await.is_err());
    assert_eq!(
        h.runtime.send(message(&h.id)).await?,
        TurnOutcome::Completed
    );
    assert!(h.backend.calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn failed_start_releases_reservation_without_returning_native_details() -> TestResult {
    let runtime = DesktopRuntime::default();
    let (sink, _events) = mpsc::unbounded_channel::<EventEnvelope>();
    let error = runtime
        .start(
            StartRequest {
                provider_id: "local".into(),
            },
            &crate::settings::Settings::new(Some("/nonexistent/igor-private-config.toml".into())),
            |_| Arc::new(sink),
        )
        .await
        .err();
    assert_eq!(error, Some(BridgeError::Provider));
    assert!(runtime.reserve().is_ok());
    Ok(())
}

#[tokio::test]
async fn cancelled_start_cannot_install_a_stale_session() -> TestResult {
    let h = harness(vec![], vec![])?;
    h.runtime.shutdown();
    let (id, cancel) = h.runtime.reserve()?;
    h.runtime.shutdown();
    assert!(cancel.is_cancelled());
    let (replacement, _) = h.runtime.reserve()?;
    assert_ne!(id, replacement);
    assert_eq!(
        h.runtime.cancel(SessionRequest { session_id: h.id }),
        Err(BridgeError::NoSession)
    );
    Ok(())
}

#[test]
fn cancelled_channel_session_can_be_closed() -> TestResult {
    let h = harness(vec![], vec![])?;
    {
        let state = h.runtime.lock()?;
        if let Some(active) = &state.active {
            active.lifetime.cancel();
        }
    }
    h.runtime.close(SessionRequest { session_id: h.id })?;
    assert!(h.runtime.reserve().is_ok());
    Ok(())
}

struct CountedBackend {
    inner: Arc<dyn ToolBackend>,
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl ToolBackend for CountedBackend {
    fn server_id(&self) -> &ServerId {
        self.inner.server_id()
    }
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, igor_core::tools::BackendError> {
        self.inner.list_tools().await
    }
    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, igor_core::tools::BackendError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.call_tool(name, arguments, cancel).await
    }
}

#[tokio::test]
async fn real_fixture_approve_once_deny_never_and_settings_snapshot_is_leased() -> TestResult {
    use crate::settings::Settings;
    use igor_config::{PermissionClass, ServerEntry};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["build", "--locked", "-q", "-p", "igor-fixture-server"])
        .current_dir(&root)
        .status()?;
    assert!(status.success());
    for (name, decision, expected) in [
        ("approve", Decision::Approve, 1),
        ("deny", Decision::Deny, 0),
    ] {
        let dir = std::env::temp_dir().join(format!("igor-bridge-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let settings = Settings::new(Some(path));
        settings
            .save_server(
                None,
                ServerEntry {
                    id: "sales".into(),
                    command: root
                        .join("target/debug/igor-fixture-server")
                        .display()
                        .to_string(),
                    args: vec![
                        "--fixture".into(),
                        root.join("crates/igor-fixture-server/fixtures/sales.json")
                            .display()
                            .to_string(),
                    ],
                    ..ServerEntry::default()
                },
            )
            .await?;
        settings
            .set_permission("sales", "create_draft_order", Some(PermissionClass::Write))
            .await?;
        let snapshot = settings.snapshot().await?;
        let server = ServerId::new("sales")?;
        let backend = Arc::new(CountedBackend {
            inner: snapshot.backends[&server].clone(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let provider = Arc::new(ScriptedProvider::new(vec![
            ScriptedTurn::tool_call(
                "draft",
                "sales__create_draft_order",
                json!({"supplier_id":"supplier-x", "delivery_date":"2026-11-03", "lines":[{"sku":"COCOA-1KG-6", "quantity":1, "rationale":"Test"}]}),
            ),
            ScriptedTurn::text("Finished"),
        ]));
        let classes = snapshot
            .descriptors
            .iter()
            .map(|tool| {
                (
                    tool.id.clone(),
                    snapshot.config.permissions().classify(&tool.id),
                )
            })
            .collect();
        let (sink, mut events) = mpsc::unbounded_channel();
        let runtime = Arc::new(DesktopRuntime::default());
        let (id, cancel) = runtime.reserve()?;
        runtime.install(
            id.clone(),
            cancel,
            Prepared {
                parts: SessionParts {
                    provider: provider.clone(),
                    registry: ToolRegistry::build(snapshot.descriptors, &classes),
                    backends: HashMap::from([(server, backend.clone() as Arc<dyn ToolBackend>)]),
                    ids: Arc::new(SequentialIds::new("fixture")),
                    events: Arc::new(sink.clone()),
                },
                config: SessionConfig::default(),
                servers: snapshot.servers,
                statuses: vec![],
                provider_id: "fake".into(),
                model: "scripted".into(),
                secrets: vec![],
                revision: snapshot.revision,
            },
            Arc::new(sink),
        )?;
        let runner = runtime.clone();
        let request = message(&id);
        let turn = tokio::spawn(async move { runner.send(request).await });
        let pending = approval(&mut events).await?;
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
        // Removing the server must not disconnect the original turn's leased backend.
        settings.remove_server("sales").await?;
        runtime.approve(ApprovalRequest {
            session_id: id.clone(),
            approval_id: pending.approval_id,
            action_hash: pending.action_hash,
            decision,
        })?;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), turn).await???,
            TurnOutcome::Completed
        );
        assert_eq!(backend.calls.load(Ordering::SeqCst), expected);
        let mut completed = 0;
        while let Ok(event) = events.try_recv() {
            if let RuntimeEvent::ToolCallCompleted { is_error, .. } = event.event {
                assert!(!is_error);
                completed += 1;
            }
        }
        assert_eq!(completed, expected);
        assert_eq!(
            runtime.send_configured(message(&id), &settings).await,
            Err(BridgeError::SettingsChanged)
        );
        provider.verify()?;
        runtime.shutdown();
    }
    Ok(())
}
