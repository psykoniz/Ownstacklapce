use anyhow::{Context, Result};
use lsp_types::{
    request::{DocumentSymbolRequest, GotoDefinition, HoverRequest, References},
    ClientCapabilities, Diagnostic, DocumentSymbolResponse, GotoDefinitionResponse, Hover,
    HoverParams, InitializeParams, InitializeResult, InitializedParams, Position,
    PublishDiagnosticsParams, ReferenceContext, ReferenceParams, TextDocumentClientCapabilities,
    TextDocumentIdentifier, TextDocumentPositionParams, Url, WindowClientCapabilities,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::{oneshot, Mutex},
    task::JoinHandle,
};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum LspMessage {
    Request {
        id: u64,
        method: String,
        params: Value,
    },
    Response {
        id: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ResponseError>,
    },
    Notification {
        method: String,
        params: Value,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, Error)]
#[error("LSP Error {code}: {message}")]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub struct LspClient {
    writer: Arc<Mutex<ChildStdin>>,
    next_id: AtomicU64,
    pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<std::result::Result<Value, ResponseError>>>>>,
    diagnostics: Arc<Mutex<HashMap<Url, Vec<Diagnostic>>>>,
    _reader_handle: JoinHandle<()>,
}

impl LspClient {
    pub async fn start(cmd: &str, args: &[String]) -> Result<Arc<Self>> {
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn LSP server")?;

        let stdin = child.stdin.take().context("Failed to open stdin")?;
        let stdout = child.stdout.take().context("Failed to open stdout")?;

        let pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<std::result::Result<Value, ResponseError>>>>> = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics = Arc::new(Mutex::new(HashMap::new()));
        
        let pending_requests_clone = pending_requests.clone();
        let diagnostics_clone = diagnostics.clone();

        let reader_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match parse_message(&mut reader).await {
                    Ok(Some(msg)) => {
                        handle_message(msg, &pending_requests_clone, &diagnostics_clone).await;
                    }
                    Ok(None) => break, // EOF
                    Err(e) => {
                        eprintln!("LSP reader error: {:?}", e);
                        break;
                    }
                }
            }
        });

        Ok(Arc::new(Self {
            writer: Arc::new(Mutex::new(stdin)),
            next_id: AtomicU64::new(1),
            pending_requests,
            diagnostics,
            _reader_handle: reader_handle,
        }))
    }

    pub async fn send_request<R>(&self, params: R::Params) -> Result<R::Result>
    where
        R: lsp_types::request::Request,
        R::Params: Serialize,
        R::Result: DeserializeOwned,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let method = R::METHOD.to_string();
        
        let params_value = serde_json::to_value(params)?;
        
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params_value,
        });

        let (tx, rx): (oneshot::Sender<std::result::Result<Value, ResponseError>>, _) = oneshot::channel();
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, tx);
        }

        self.send_raw(request.to_string()).await?;

        // Wait for response
        let response = rx.await.context("Sender dropped")??; 
        
        serde_json::from_value(response).context("Failed to deserialize response")
    }

    pub async fn send_notification<N>(&self, params: N::Params) -> Result<()>
    where
        N: lsp_types::notification::Notification,
        N::Params: Serialize,
    {
        let method = N::METHOD.to_string();
        let params_value = serde_json::to_value(params)?;
        
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params_value,
        });

        self.send_raw(notification.to_string()).await
    }

    async fn send_raw(&self, body: String) -> Result<()> {
        let content_length = body.len();
        let message = format!("Content-Length: {}\r\n\r\n{}", content_length, body);
        
        let mut writer = self.writer.lock().await;
        writer.write_all(message.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    pub async fn initialize(&self, root_uri: Url) -> Result<InitializeResult> {
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(root_uri.clone()),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    ..Default::default()
                }),
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = self.send_request::<lsp_types::request::Initialize>(params).await?;
        
        self.send_notification::<lsp_types::notification::Initialized>(InitializedParams {})
            .await?;

        Ok(result)
    }

    pub async fn text_document_did_open(&self, params: lsp_types::DidOpenTextDocumentParams) -> Result<()> {
        self.send_notification::<lsp_types::notification::DidOpenTextDocument>(params).await
    }

    pub async fn get_diagnostics(&self, uri: &Url) -> Option<Vec<Diagnostic>> {
        let diagnostics = self.diagnostics.lock().await;
        diagnostics.get(uri).cloned()
    }

    pub async fn goto_definition(&self, uri: Url, line: u32, character: u32) -> Result<Option<GotoDefinitionResponse>> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self.send_request::<GotoDefinition>(params).await?;
        Ok(result)
    }

    pub async fn find_references(&self, uri: Url, line: u32, character: u32) -> Result<Option<Vec<lsp_types::Location>>> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            context: ReferenceContext { include_declaration: true },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self.send_request::<References>(params).await?;
        Ok(result)
    }

    pub async fn document_symbol(&self, uri: Url) -> Result<Option<DocumentSymbolResponse>> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self.send_request::<DocumentSymbolRequest>(params).await?;
        Ok(result)
    }

    pub async fn hover(&self, uri: Url, line: u32, character: u32) -> Result<Option<Hover>> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
        };

        let result = self.send_request::<HoverRequest>(params).await?;
        Ok(result)
    }
}

// Helper structs for params not exported directly or needed composition
use lsp_types::{GotoDefinitionParams, DocumentSymbolParams};

async fn parse_message<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length = 0;
    
    // Read headers
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Ok(None);
        }

        let line = line.trim();
        if line.is_empty() {
            break; // End of headers
        }

        if let Some(rest) = line.strip_prefix("Content-Length: ") {
            content_length = rest.parse().context("Invalid Content-Length")?;
        }
    }

    if content_length == 0 {
        return Ok(None);
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await?;
    
    let value: Value = serde_json::from_slice(&body)?;
    Ok(Some(value))
}

async fn handle_message(
    msg: Value,
    pending_requests: &Arc<Mutex<HashMap<u64, oneshot::Sender<std::result::Result<Value, ResponseError>>>>>,
    diagnostics: &Arc<Mutex<HashMap<Url, Vec<Diagnostic>>>>,
) {
    if let Some(id_val) = msg.get("id") {
        if let Some(id) = id_val.as_u64() {
           if let Some(method) = msg.get("method") {
               // Request from server
               eprintln!("Received request from server: {}", method);
           } else {
               // Response
               let mut pending = pending_requests.lock().await;
               if let Some(tx) = pending.remove(&id) {
                   if let Some(error) = msg.get("error") {
                        if let Ok(err_obj) = serde_json::from_value::<ResponseError>(error.clone()) {
                            let _ = tx.send(Err(err_obj));
                        }
                   } else if let Some(result) = msg.get("result") {
                       let _ = tx.send(Ok(result.clone()));
                   } else {
                       let _ = tx.send(Ok(Value::Null));
                   }
               }
           }
        }
    } else {
        // Notification
        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            if method == "textDocument/publishDiagnostics" {
                if let Some(params) = msg.get("params") {
                    if let Ok(diag_params) = serde_json::from_value::<PublishDiagnosticsParams>(params.clone()) {
                        let mut diags = diagnostics.lock().await;
                        diags.insert(diag_params.uri, diag_params.diagnostics);
                    }
                }
            }
        }
    }
}
