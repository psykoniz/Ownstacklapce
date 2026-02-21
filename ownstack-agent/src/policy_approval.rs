use lapce_rpc::ownstack::OwnStackRpc;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tokio::time::{Duration, timeout};
use tracing::warn;

pub type RpcSink = Arc<dyn Fn(OwnStackRpc) + Send + Sync>;

/// Coordinates "Ask mode" approvals with the IDE UI.
///
/// Design constraints:
/// - Only one outstanding approval is supported for now (simplifies UX + avoids races).
/// - The stdin reader task must handle `OwnStackRpc::PolicyResponse` concurrently with
///   agent work; otherwise an approval request would deadlock the main loop.
pub struct PolicyApprovalManager {
    sink: RpcSink,
    pending: Mutex<Option<oneshot::Sender<bool>>>,
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
    pub async fn request(&self, command: String, reason: String) -> bool {
        let (tx, rx) = oneshot::channel::<bool>();
        {
            let mut pending = self.pending.lock().await;
            if pending.is_some() {
                warn!("Policy approval requested while another approval is pending");
                return false;
            }
            *pending = Some(tx);
        }

        (self.sink)(OwnStackRpc::PolicyPrompt { command, reason });

        // Avoid hanging forever if the UI never responds.
        const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);
        match timeout(APPROVAL_TIMEOUT, rx).await {
            Ok(Ok(approved)) => approved,
            Ok(Err(_closed)) => false,
            Err(_timeout) => {
                let mut pending = self.pending.lock().await;
                pending.take();
                false
            }
        }
    }

    /// Resolve a pending approval request.
    pub async fn resolve(&self, approved: bool) {
        let tx = {
            let mut pending = self.pending.lock().await;
            pending.take()
        };

        if let Some(tx) = tx {
            let _ = tx.send(approved);
        } else {
            warn!("Received PolicyResponse but no approval is pending");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[tokio::test]
    async fn request_emits_prompt_and_resolves() {
        let sent = Arc::new(StdMutex::new(Vec::<OwnStackRpc>::new()));
        let sent_sink = sent.clone();
        let sink: RpcSink = Arc::new(move |rpc| {
            sent_sink.lock().unwrap().push(rpc);
        });

        let mgr = Arc::new(PolicyApprovalManager::new(sink));
        let mgr_req = mgr.clone();

        let handle = tokio::spawn(async move {
            mgr_req
                .request("git push origin main".to_string(), "test".to_string())
                .await
        });

        // Ensure request() had a chance to send the prompt and park.
        tokio::time::sleep(Duration::from_millis(10)).await;
        mgr.resolve(true).await;

        let approved = handle.await.unwrap();
        assert!(approved);

        let msgs = sent.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], OwnStackRpc::PolicyPrompt { .. }));
    }

    #[tokio::test]
    async fn resolve_without_pending_is_safe() {
        let sink: RpcSink = Arc::new(|_rpc| {});
        let mgr = PolicyApprovalManager::new(sink);
        mgr.resolve(false).await;
    }
}

