use lapce_rpc::ownstack::OwnStackRpc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tracing::warn;

/// Simple monotonic correlation ID generator (no external crate required).
static CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(1);

fn new_correlation_id() -> String {
    let n = CORRELATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("policy-{}-{}", ts, n)
}

pub type RpcSink = Arc<dyn Fn(OwnStackRpc) + Send + Sync>;

/// Default time the UI is given to respond before auto-deny.
pub const POLICY_PROMPT_TIMEOUT_SECS: u32 = 15;

struct PendingApproval {
    correlation_id: String,
    sender: oneshot::Sender<bool>,
}

/// Coordinates "Ask mode" approvals with the IDE UI.
///
/// Design constraints:
/// - Only one outstanding approval is supported for now (simplifies UX + avoids races).
/// - The stdin reader task must handle `OwnStackRpc::PolicyResponse` concurrently with
///   agent work; otherwise an approval request would deadlock the main loop.
pub struct PolicyApprovalManager {
    sink: RpcSink,
    pending: Mutex<Option<PendingApproval>>,
}

impl PolicyApprovalManager {
    pub fn new(sink: RpcSink) -> Self {
        Self {
            sink,
            pending: Mutex::new(None),
        }
    }

    /// Request an approval from the IDE UI.
    ///
    /// Returns `true` if the user approved, `false` on deny, timeout, or protocol errors.
    /// `cwd` is displayed in the modal for context.
    pub async fn request(
        &self,
        command: String,
        reason: String,
        cwd: Option<String>,
    ) -> bool {
        let (tx, rx) = oneshot::channel::<bool>();
        let correlation_id = new_correlation_id();

        {
            let mut pending = self.pending.lock().await;
            if pending.is_some() {
                warn!("Policy approval requested while another approval is pending");
                return false;
            }
            *pending = Some(PendingApproval {
                correlation_id: correlation_id.clone(),
                sender: tx,
            });
        }

        (self.sink)(OwnStackRpc::PolicyPrompt {
            command,
            reason,
            cwd,
            correlation_id,
            timeout_secs: POLICY_PROMPT_TIMEOUT_SECS,
        });

        let deadline = Duration::from_secs(u64::from(POLICY_PROMPT_TIMEOUT_SECS));
        match timeout(deadline, rx).await {
            Ok(Ok(approved)) => approved,
            Ok(Err(_closed)) => {
                // Channel dropped — treat as deny
                let mut pending = self.pending.lock().await;
                pending.take();
                false
            }
            Err(_elapsed) => {
                // Auto-deny on timeout (mirrors UI timer)
                let mut pending = self.pending.lock().await;
                pending.take();
                warn!(
                    "Policy prompt timed out after {}s — auto-deny",
                    POLICY_PROMPT_TIMEOUT_SECS
                );
                false
            }
        }
    }

    /// Resolve a pending approval request.
    ///
    /// `correlation_id` must match the one from the outstanding `PolicyPrompt`.
    /// Mismatched ids are ignored and logged.
    pub async fn resolve(&self, approved: bool, correlation_id: &str) {
        let ap = {
            let mut pending = self.pending.lock().await;
            match pending.as_ref() {
                Some(p) if p.correlation_id == correlation_id => pending.take(),
                Some(p) => {
                    warn!(
                        "PolicyResponse correlation_id mismatch: got '{}', expected '{}'",
                        correlation_id, p.correlation_id
                    );
                    return;
                }
                None => {
                    warn!("Received PolicyResponse but no approval is pending");
                    return;
                }
            }
        };

        if let Some(ap) = ap {
            let _ = ap.sender.send(approved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn make_sink() -> (RpcSink, Arc<StdMutex<Vec<OwnStackRpc>>>) {
        let sent = Arc::new(StdMutex::new(Vec::<OwnStackRpc>::new()));
        let sent_clone = sent.clone();
        let sink: RpcSink = Arc::new(move |rpc| {
            sent_clone.lock().unwrap().push(rpc);
        });
        (sink, sent)
    }

    // F3-a: Allow → continue
    #[tokio::test]
    async fn request_emits_prompt_and_resolves_allow() {
        let (sink, sent) = make_sink();
        let mgr = Arc::new(PolicyApprovalManager::new(sink));
        let mgr_req = mgr.clone();

        let handle = tokio::spawn(async move {
            mgr_req
                .request(
                    "git push origin main".to_string(),
                    "publishing code".to_string(),
                    Some("/workspace/project".to_string()),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;

        let correlation_id = {
            let msgs = sent.lock().unwrap();
            assert_eq!(msgs.len(), 1);
            match &msgs[0] {
                OwnStackRpc::PolicyPrompt {
                    command,
                    timeout_secs,
                    correlation_id,
                    cwd,
                    ..
                } => {
                    assert_eq!(command, "git push origin main");
                    assert_eq!(*timeout_secs, POLICY_PROMPT_TIMEOUT_SECS);
                    assert!(!correlation_id.is_empty());
                    assert_eq!(cwd.as_deref(), Some("/workspace/project"));
                    correlation_id.clone()
                }
                other => panic!("Expected PolicyPrompt, got {:?}", other),
            }
        };

        mgr.resolve(true, &correlation_id).await;
        let approved = handle.await.unwrap();
        assert!(approved, "Expected allow to return true");
    }

    // F3-b: Deny → returns false
    #[tokio::test]
    async fn request_emits_prompt_and_resolves_deny() {
        let (sink, sent) = make_sink();
        let mgr = Arc::new(PolicyApprovalManager::new(sink));
        let mgr_req = mgr.clone();

        let handle = tokio::spawn(async move {
            mgr_req
                .request(
                    "npm publish".to_string(),
                    "publishing package".to_string(),
                    None,
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        let correlation_id = {
            let msgs = sent.lock().unwrap();
            match &msgs[0] {
                OwnStackRpc::PolicyPrompt { correlation_id, .. } => {
                    correlation_id.clone()
                }
                _ => panic!("Expected PolicyPrompt"),
            }
        };

        mgr.resolve(false, &correlation_id).await;
        let approved = handle.await.unwrap();
        assert!(!approved, "Expected deny to return false");
    }

    // F3-c: Mismatched correlation_id is safely ignored
    #[tokio::test]
    async fn mismatched_correlation_id_is_ignored() {
        let (sink, _sent) = make_sink();
        let mgr = Arc::new(PolicyApprovalManager::new(sink));
        let mgr_req = mgr.clone();

        let handle = tokio::spawn(async move {
            mgr_req
                .request("git push".to_string(), "pushing code".to_string(), None)
                .await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        // Wrong id — should be silently ignored, pending stays active
        mgr.resolve(true, "wrong-id-totally-invalid").await;
        handle.abort();
    }

    #[tokio::test]
    async fn resolve_without_pending_is_safe() {
        let sink: RpcSink = Arc::new(|_rpc| {});
        let mgr = PolicyApprovalManager::new(sink);
        mgr.resolve(false, "no-such-id").await;
    }

    #[tokio::test]
    async fn double_request_is_denied_immediately() {
        let (sink, _sent) = make_sink();
        let mgr = Arc::new(PolicyApprovalManager::new(sink));
        let mgr2 = mgr.clone();

        let first = tokio::spawn(async move {
            mgr2.request("cmd1".to_string(), "r1".to_string(), None)
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let second_result = mgr
            .request("cmd2".to_string(), "r2".to_string(), None)
            .await;
        assert!(!second_result, "Second concurrent request should be denied");

        first.abort();
    }

    // Confirm constant is in the expected range for the spec (15s)
    #[test]
    fn policy_timeout_constant_is_fifteen_seconds() {
        assert_eq!(POLICY_PROMPT_TIMEOUT_SECS, 15);
    }
}
