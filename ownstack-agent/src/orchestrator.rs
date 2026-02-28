//! Agent Orchestrator — Multi-Agent Architecture
//!
//! Manages the agent loop with Planner → Worker → Critic pattern.
//! Enforces budgets and security at every step.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::context::ContextManager;
use crate::provider::{FinishReason, LlmMessage, LlmProvider, ToolDefinition};
use crate::toolkits::{ToolResult, Toolkit, ToolkitError};

// ─── Tool-args Safety Constants ────────────────────────────────────

/// Maximum allowed byte size for raw tool-call arguments.
/// If the model sends more than this it is almost certainly truncated.
const TOOL_ARGS_MAX_BYTES: usize = 16 * 1024; // 16 KB

/// How many times the *same* parse-error signature may repeat before
/// the orchestrator aborts with a controlled error instead of looping.
const REPEATED_PARSE_ERROR_LIMIT: u32 = 3;

// ─── Orchestrator Error ─────────────────────────────────────────────

/// Structured error variants returned by the orchestrator.
#[derive(Debug, Clone)]
pub enum OrchestratorError {
    /// Tool arguments are repeated-invalid: abort before timeout.
    RepeatedInvalidToolArgs {
        model: String,
        tool_name: String,
        count: u32,
        /// Safe excerpt of the offending input (≤200 chars).
        input_excerpt: String,
    },
    /// Tool arguments exceed the byte-size limit.
    ToolArgsTooLarge {
        tool_name: String,
        actual_bytes: usize,
        limit_bytes: usize,
    },
    /// Generic orchestrator error (budget, LLM, …).
    General(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepeatedInvalidToolArgs { model, tool_name, count, input_excerpt } => write!(
                f,
                "Orchestrator aborted: tool '{}' received invalid/truncated JSON args \
                 {} time(s) in a row (model: {}). \
                 Excerpt: {:?}. \
                 Recommendation: switch model or reduce tool payload.",
                tool_name, count, model, input_excerpt
            ),
            Self::ToolArgsTooLarge { tool_name, actual_bytes, limit_bytes } => write!(
                f,
                "Tool '{}' args too large ({} bytes > {} byte limit), \
                 likely truncated by model/provider.",
                tool_name, actual_bytes, limit_bytes
            ),
            Self::General(s) => write!(f, "{}", s),
        }
    }
}

impl From<String> for OrchestratorError {
    fn from(s: String) -> Self {
        Self::General(s)
    }
}

// ─── Parse-Error Signature ──────────────────────────────────────────

/// Identifies a repeated tool-parse failure.
/// Two failures have the same signature if (tool_name, error_kind, args_prefix)
/// are identical, which indicates a stuck model loop.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParseErrSig {
    tool_name: String,
    error_kind: String,
    /// First 64 chars of the offending raw args string.
    args_prefix: String,
}

/// Tracks consecutive parse-error repetitions.
#[derive(Debug, Default)]
struct ParseErrorTracker {
    last_sig: Option<ParseErrSig>,
    count: u32,
}

impl ParseErrorTracker {
    /// Record one occurrence of `sig`.  Returns the new consecutive count.
    fn record(&mut self, sig: ParseErrSig) -> u32 {
        if self.last_sig.as_ref() == Some(&sig) {
            self.count += 1;
        } else {
            self.last_sig = Some(sig);
            self.count = 1;
        }
        self.count
    }

    /// Reset on a successful parse.
    fn reset(&mut self) {
        self.last_sig = None;
        self.count = 0;
    }
}

// ─── Helper: validate tool args ────────────────────────────────────

/// Returns `Ok(parsed_value)` or an `OrchestratorError` that the caller
/// should record in the parse-error tracker and potentially abort on.
fn validate_and_parse_tool_args(
    tool_name: &str,
    args_str: &str,
) -> Result<serde_json::Value, OrchestratorError> {
    // 1. Size guard
    if args_str.len() > TOOL_ARGS_MAX_BYTES {
        return Err(OrchestratorError::ToolArgsTooLarge {
            tool_name: tool_name.to_string(),
            actual_bytes: args_str.len(),
            limit_bytes: TOOL_ARGS_MAX_BYTES,
        });
    }

    // 2. Parse guard
    serde_json::from_str(args_str).map_err(|e| {
        // Classify the error kind coarsely for signature matching
        let error_kind = if e.is_eof() {
            "unexpected_eof"
        } else if e.is_syntax() {
            "syntax_error"
        } else if e.is_data() {
            "data_error"
        } else {
            "other"
        };

        let args_prefix: String = args_str.chars().take(64).collect();
        // We piggy-back the signature data in a temporary error – the
        // caller decides whether to abort or continue.
        OrchestratorError::RepeatedInvalidToolArgs {
            model: String::new(), // filled in by caller
            tool_name: tool_name.to_string(),
            count: 0,             // filled in by caller
            input_excerpt: format!("{}: {:?}", error_kind, args_prefix),
        }
    })
}

// ─── Budget ────────────────────────────────────────────────────────

/// Budget limits per GEMINI.md §6.8
#[derive(Debug, Clone)]
pub struct AgentBudget {
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub max_llm_calls: u32,
    pub max_files_modified: u32,
    pub max_duration_minutes: u32,
    pub max_consecutive_failures: u32,
}

impl Default for AgentBudget {
    fn default() -> Self {
        Self {
            max_steps: 50,
            max_tool_calls: 30,
            max_llm_calls: 100,
            max_files_modified: 20,
            max_duration_minutes: 30,
            max_consecutive_failures: 3,
        }
    }
}

#[derive(Debug, Default)]
struct BudgetCounters {
    steps: u32,
    tool_calls: u32,
    llm_calls: u32,
    consecutive_failures: u32,
}

// ─── Mission System ────────────────────────────────────────────────

/// A step in a mission plan
#[derive(Debug, Clone)]
pub struct MissionStep {
    pub description: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

/// A multi-step mission
#[derive(Debug, Clone)]
pub struct Mission {
    pub goal: String,
    pub steps: Vec<MissionStep>,
}

impl Mission {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            steps: Vec::new(),
        }
    }

    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        (done, self.steps.len())
    }
}

// ─── Multi-Agent Roles ─────────────────────────────────────────────

const PLANNER_PROMPT: &str = r#"You are the Planner agent. Your role is to decompose a user request into clear, actionable steps.

Output a JSON array of steps, each with a "description" field. Example:
[
  {"description": "Read the current implementation of the auth module"},
  {"description": "Identify the bug in the login flow"},
  {"description": "Fix the validation logic"},
  {"description": "Run tests to verify the fix"}
]

Be specific and practical. Each step should be achievable with available tools."#;

const CRITIC_PROMPT: &str = r#"You are the Critic agent. Your role is to review the Worker's output and determine if the task was completed correctly.

Analyze the output and respond with a JSON object:
{
  "approved": true/false,
  "feedback": "explanation of any issues or confirmation of success",
  "suggestions": ["optional list of improvements"]
}

Be thorough but fair. Only reject if there are genuine issues."#;

// ─── Orchestrator ──────────────────────────────────────────────────

/// Multi-agent orchestrator with Planner → Worker → Critic pattern
pub struct AgentOrchestrator {
    provider: Arc<dyn LlmProvider>,
    toolkits: Vec<Arc<dyn Toolkit>>,
    context: ContextManager,
    budget: AgentBudget,
    counters: BudgetCounters,
    #[allow(dead_code)]
    workspace: PathBuf,
    memory: crate::project_memory::ProjectMemory,
    index: crate::index::SemanticIndex,
    current_mission: Option<Mission>,
    started_at: Option<Instant>,
    /// Tracks consecutive identical tool-parse errors for anti-loop detection.
    parse_error_tracker: ParseErrorTracker,
}

impl AgentOrchestrator {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        workspace: PathBuf,
        max_context_tokens: usize,
    ) -> Self {
        let memory = crate::project_memory::ProjectMemory::new(workspace.clone());
        let index = crate::index::SemanticIndex::new(workspace.clone());
        Self {
            provider,
            toolkits: Vec::new(),
            context: ContextManager::new(max_context_tokens),
            budget: AgentBudget::default(),
            counters: BudgetCounters::default(),
            workspace,
            memory,
            index,
            current_mission: None,
            started_at: None,
            parse_error_tracker: ParseErrorTracker::default(),
        }
    }

    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.context.set_system_prompt(prompt);
    }

    pub fn set_budget(&mut self, budget: AgentBudget) {
        self.budget = budget;
    }

    pub fn register_toolkit(&mut self, toolkit: Arc<dyn Toolkit>) {
        self.toolkits.push(toolkit);
    }

    pub fn current_mission(&self) -> Option<&Mission> {
        self.current_mission.as_ref()
    }

    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.toolkits
            .iter()
            .flat_map(|tk| tk.tools())
            .map(|t| ToolDefinition {
                name: t.name,
                description: t.description,
                parameters: t.parameters,
            })
            .collect()
    }

    pub async fn execute_tool_shared(
        toolkits: &[Arc<dyn Toolkit>],
        name: &str,
        args: serde_json::Value,
    ) -> ToolResult {
        for toolkit in toolkits {
            match toolkit.execute(name, args.clone()).await {
                Ok(result) => return result,
                Err(e) => {
                    match e {
                        ToolkitError::ToolNotFound(_) => continue,
                        _ => return ToolResult::failure(format!("{}", e), None),
                    }
                }
            }
        }
        ToolResult::failure(format!("Tool not found: {}", name), None)
    }

    pub async fn execute_tool(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> ToolResult {
        self.counters.tool_calls += 1;
        Self::execute_tool_shared(&self.toolkits, name, args).await
    }

    fn route_model(&self, task_type: &str) -> Option<String> {
        let provider_name = self.provider.name();
        match (provider_name, task_type) {
            ("anthropic", "planning") => Some("claude-sonnet-4-6".to_string()),
            ("anthropic", "critique") => Some("claude-sonnet-4-6".to_string()),
            ("anthropic", "worker") => Some("claude-sonnet-4-6".to_string()),
            ("anthropic", "fast") => Some("claude-haiku-4-5-20251001".to_string()),
            ("openrouter", "planning") => Some("anthropic/claude-sonnet-4-6".to_string()),
            ("openrouter", "critique") => Some("anthropic/claude-sonnet-4-6".to_string()),
            ("openrouter", "worker") => Some("anthropic/claude-sonnet-4-6".to_string()),
            ("openrouter", "fast") => Some("anthropic/claude-haiku-4-5-20251001".to_string()),
            _ => None, // use default from config
        }
    }

    fn check_budget(&self) -> Option<String> {
        if self.counters.steps >= self.budget.max_steps {
            Some(format!(
                "max_steps ({}/{})",
                self.counters.steps, self.budget.max_steps
            ))
        } else if self.counters.tool_calls >= self.budget.max_tool_calls {
            Some(format!(
                "max_tool_calls ({}/{})",
                self.counters.tool_calls, self.budget.max_tool_calls
            ))
        } else if self.counters.llm_calls >= self.budget.max_llm_calls {
            Some(format!(
                "max_llm_calls ({}/{})",
                self.counters.llm_calls, self.budget.max_llm_calls
            ))
        } else if self.counters.consecutive_failures
            >= self.budget.max_consecutive_failures
        {
            Some(format!(
                "max_consecutive_failures ({})",
                self.counters.consecutive_failures
            ))
        } else if let Some(started_at) = self.started_at {
            let max_secs = (self.budget.max_duration_minutes as u64) * 60;
            if started_at.elapsed().as_secs() >= max_secs {
                Some(format!(
                    "max_duration_minutes ({}m)",
                    self.budget.max_duration_minutes
                ))
            } else {
                None
            }
        } else {
            None
        }
    }

    // ─── Planning Phase ────────────────────────────────────────────

    /// Use the Planner agent to decompose a task
    pub async fn plan(&mut self, user_goal: &str) -> Result<Mission, String> {
        info!("Planner: decomposing task: {}", user_goal);
        self.counters.llm_calls += 1;

        let messages = vec![
            LlmMessage::system(PLANNER_PROMPT),
            LlmMessage::user(user_goal),
        ];

        let model = self.route_model("planning");
        let response = self
            .provider
            .complete(messages, None, model)
            .await
            .map_err(|e| format!("Planner error: {}", e))?;

        let content = response.content.unwrap_or_default();

        // Parse steps from JSON response
        let steps: Vec<serde_json::Value> = serde_json::from_str(&content)
            .map_err(|_| format!("Planner returned invalid JSON: {}", content))?;

        let mut mission = Mission::new(user_goal);
        for step_val in steps {
            if let Some(desc) = step_val.get("description").and_then(|v| v.as_str())
            {
                mission.steps.push(MissionStep {
                    description: desc.to_string(),
                    status: StepStatus::Pending,
                });
            }
        }

        info!("Planner: created {} steps", mission.steps.len());
        self.current_mission = Some(mission.clone());
        Ok(mission)
    }

    // ─── Critic Phase ──────────────────────────────────────────────

    /// Use the Critic agent to review output
    pub async fn critique(
        &mut self,
        task: &str,
        output: &str,
    ) -> Result<CriticResult, String> {
        info!("Critic: reviewing output for: {}", task);
        self.counters.llm_calls += 1;

        let prompt = format!(
            "Task: {}\n\nWorker output:\n{}\n\nReview this output.",
            task, output
        );

        let messages =
            vec![LlmMessage::system(CRITIC_PROMPT), LlmMessage::user(prompt)];

        let model = self.route_model("critique");
        let response = self
            .provider
            .complete(messages, None, model)
            .await
            .map_err(|e| format!("Critic error: {}", e))?;

        let content = response.content.unwrap_or_default();

        let result: CriticResult =
            serde_json::from_str(&content).unwrap_or(CriticResult {
                approved: true,
                feedback: content,
                suggestions: Vec::new(),
            });

        Ok(result)
    }

    // ─── Worker Phase (main agent loop) ────────────────────────────

    /// Process a single prompt through the agent loop with streaming
    pub async fn stream_process<F, M>(
        &mut self,
        user_prompt: &str,
        mut on_chunk: F,
        mut on_mission: M,
    ) -> Result<String, String>
    where
        F: FnMut(crate::provider::StreamChunk) + Send,
        M: FnMut(Mission) + Send,
    {
        use futures::StreamExt;

        self.started_at = Some(Instant::now());

        // Ensure index is initialized
        if let Err(e) = self.index.init().await {
            debug!("SemanticIndex init failed (likely missing files): {}", e);
        }

        // Augment system prompt with project memory and semantic index
        let project_rules = self.memory.to_system_prompt();
        
        // Phase 11: Semantic Retrieval
        let mut rag_context = String::new();
        if let Ok(snippets) = self.index.search(user_prompt, 5).await {
            if !snippets.is_empty() {
                rag_context = "\n## Relevant Code Context\n".to_string();
                for snip in snippets {
                    rag_context.push_str(&format!(
                        "\nFile: {} (Lines {}-{})\n```\n{}\n```\n",
                        snip.path, snip.start_line, snip.end_line, snip.content
                    ));
                }
            }
        }

        if !project_rules.is_empty() || !rag_context.is_empty() {
            let base_prompt = self
                .context
                .get_messages()
                .get(0)
                .map(|m| m.content.clone())
                .unwrap_or_default();
            let augmented = format!("{}\n\n{}\n\n{}", base_prompt, project_rules, rag_context);
            self.context.set_system_prompt(augmented);
        }

        self.context.add_message(LlmMessage::user(user_prompt));

        // Create mission if in Plan mode or if specified
        if let Ok(mission) = self.plan(user_prompt).await {
            on_mission(mission);
        }

        loop {
            self.counters.steps += 1;

            if let Some(reason) = self.check_budget() {
                warn!("Budget exceeded: {}", reason);
                return Err(format!("⚠️ Agent stopped: {}", reason));
            }

            self.counters.llm_calls += 1;
            let tools = Some(self.get_tool_definitions());
            let messages = self.context.get_messages();

            debug!(
                "Worker step {} (LLM call #{}) [STREAMING]",
                self.counters.steps, self.counters.llm_calls
            );

            let mut full_content = String::new();
            let mut tool_calls_deltas: std::collections::HashMap<
                usize,
                (Option<String>, Option<String>, String),
            > = std::collections::HashMap::new();
            let mut final_finish_reason = None;
            let mut _final_usage = None;

            let model = self.route_model("worker");
            let mut stream = self
                .provider
                .stream(messages.clone(), tools, model)
                .await
                .map_err(|e| format!("LLM Stream error: {}", e))?;

            while let Some(chunk_result) = stream.next().await {
                let chunk =
                    chunk_result.map_err(|e| format!("Stream error: {}", e))?;

                // Emit to UI
                on_chunk(chunk.clone());

                if let Some(delta) = chunk.delta_content {
                    full_content.push_str(&delta);
                }

                for delta in chunk.delta_tool_calls {
                    let entry = tool_calls_deltas.entry(delta.index).or_insert((
                        None,
                        None,
                        String::new(),
                    ));
                    if delta.id.is_some() {
                        entry.0 = delta.id;
                    }
                    if delta.name.is_some() {
                        entry.1 = delta.name;
                    }
                    if let Some(arg) = delta.arguments_delta {
                        entry.2.push_str(&arg);
                    }
                }

                if chunk.finish_reason.is_some() {
                    final_finish_reason = chunk.finish_reason;
                }
                if chunk.usage.is_some() {
                    _final_usage = chunk.usage;
                }
            }

            let finish_reason = final_finish_reason.unwrap_or(FinishReason::Stop);

            match finish_reason {
                FinishReason::Stop => {
                    self.context
                        .add_message(LlmMessage::assistant(&full_content));
                    self.counters.consecutive_failures = 0;
                    info!(
                        "Worker completed: {} steps, {} tool calls",
                        self.counters.steps, self.counters.tool_calls
                    );
                    return Ok(full_content);
                }
                FinishReason::ToolCalls => {
                    let mut tool_calls = Vec::new();
                    let mut sorted_indices: Vec<_> =
                        tool_calls_deltas.keys().collect();
                    sorted_indices.sort();

                    for idx in sorted_indices {
                        let (id, name, args_str) =
                            tool_calls_deltas.get(idx).unwrap();
                        let tool_name = name.clone().unwrap_or_default();

                        let arguments = match validate_and_parse_tool_args(&tool_name, args_str) {
                            Ok(v) => {
                                self.parse_error_tracker.reset();
                                v
                            }
                            Err(parse_err) => {
                                // Classify and build the signature for dedup
                                let (error_kind, args_prefix) = match &parse_err {
                                    OrchestratorError::ToolArgsTooLarge { .. } => (
                                        "too_large".to_string(),
                                        args_str.chars().take(64).collect(),
                                    ),
                                    OrchestratorError::RepeatedInvalidToolArgs { input_excerpt, .. } => (
                                        input_excerpt.split(':').next().unwrap_or("parse_error").to_string(),
                                        args_str.chars().take(64).collect(),
                                    ),
                                    _ => ("unknown".to_string(), String::new()),
                                };

                                let sig = ParseErrSig {
                                    tool_name: tool_name.clone(),
                                    error_kind,
                                    args_prefix,
                                };
                                let count = self.parse_error_tracker.record(sig);
                                self.counters.consecutive_failures += 1;

                                if count >= REPEATED_PARSE_ERROR_LIMIT {
                                    let excerpt: String = args_str.chars().take(200).collect();
                                    let err = OrchestratorError::RepeatedInvalidToolArgs {
                                        model: self.provider.name().to_string(),
                                        tool_name: tool_name.clone(),
                                        count,
                                        input_excerpt: excerpt,
                                    };
                                    warn!("{}", err);
                                    return Err(err.to_string());
                                }

                                warn!(
                                    "Tool '{}' parse error (attempt {}/{}): {}",
                                    tool_name, count, REPEATED_PARSE_ERROR_LIMIT, parse_err
                                );
                                // Use Null as fallback — tool will return an error
                                serde_json::Value::Null
                            }
                        };

                        tool_calls.push(crate::provider::ToolCall {
                            id: id
                                .clone()
                                .unwrap_or_else(|| format!("call_{}", idx)),
                            name: tool_name,
                            arguments,
                        });
                    }

                    let mut assistant_msg = LlmMessage::assistant(&full_content);
                    assistant_msg.tool_calls = Some(tool_calls.clone());
                    self.context.add_message(assistant_msg);

                    let toolkits = self.toolkits.clone();
                    let futures = tool_calls.into_iter().map(|tool_call| {
                        let tks = toolkits.clone();
                        async move {
                            debug!("Executing (parallel): {} ({})", tool_call.name, tool_call.arguments);
                            let result = Self::execute_tool_shared(&tks, &tool_call.name, tool_call.arguments.clone()).await;
                            (tool_call, result)
                        }
                    });

                    let results = futures::future::join_all(futures).await;

                    for (tool_call, result) in results {
                        self.counters.tool_calls += 1;
                        if !result.success {
                            self.counters.consecutive_failures += 1;
                        } else {
                            self.counters.consecutive_failures = 0;
                        }

                        let result_msg = if let Some(image_data) = result.metadata.get("image_data") {
                            let media_type = result.metadata.get("media_type")
                                .cloned()
                                .unwrap_or_else(|| "image/png".to_string());
                            
                            LlmMessage {
                                role: crate::provider::Role::Tool,
                                content: crate::provider::MessageContent::Parts(vec![
                                    crate::provider::ContentPart::Text { text: serde_json::to_string(&result).unwrap_or_default() },
                                    crate::provider::ContentPart::Image {
                                        source: crate::provider::ImageSource {
                                            type_: "base64".to_string(),
                                            media_type,
                                            data: image_data.clone(),
                                        }
                                    }
                                ]),
                                tool_call_id: Some(tool_call.id.clone()),
                                tool_calls: None,
                            }
                        } else {
                            let result_json = serde_json::to_string(&result).unwrap_or_default();
                            LlmMessage::tool_result(&tool_call.id, result_json)
                        };
                        self.context.add_message(result_msg);
                    }
                    // Loop back for next interaction after tool results
                }
                FinishReason::Length => {
                    return Err("Response truncated (max tokens)".to_string());
                }
                FinishReason::Error => {
                    self.counters.consecutive_failures += 1;
                    return Err("LLM returned error".to_string());
                }
            }
        }
    }

    /// Process a single prompt through the agent loop (Worker role)
    pub async fn process(&mut self, user_prompt: &str) -> Result<String, String> {
        self.started_at = Some(Instant::now());
        self.context.add_message(LlmMessage::user(user_prompt));

        loop {
            self.counters.steps += 1;

            if let Some(reason) = self.check_budget() {
                warn!("Budget exceeded: {}", reason);
                return Err(format!("⚠️ Agent stopped: {}", reason));
            }

            self.counters.llm_calls += 1;
            let tools = Some(self.get_tool_definitions());
            let messages = self.context.get_messages();

            debug!(
                "Worker step {} (LLM call #{})",
                self.counters.steps, self.counters.llm_calls
            );

            let model = self.route_model("worker");
            let response = self
                .provider
                .complete(messages, tools, model)
                .await
                .map_err(|e| format!("LLM error: {}", e))?;

            match response.finish_reason {
                FinishReason::Stop => {
                    let content = response.content.unwrap_or_default();
                    self.context.add_message(LlmMessage::assistant(&content));
                    self.counters.consecutive_failures = 0;
                    info!(
                        "Worker completed: {} steps, {} tool calls",
                        self.counters.steps, self.counters.tool_calls
                    );
                    return Ok(content);
                }
                FinishReason::ToolCalls => {
                    let mut assistant_msg = LlmMessage::assistant(
                        response.content.clone().unwrap_or_default(),
                    );
                    assistant_msg.tool_calls = Some(response.tool_calls.clone());
                    self.context.add_message(assistant_msg);

                    for tool_call in response.tool_calls {
                        debug!(
                            "Executing: {} ({})",
                            tool_call.name, tool_call.arguments
                        );

                        // Size guard on already-parsed args (re-serialise to measure)
                        let args_serialised = tool_call.arguments.to_string();
                        if args_serialised.len() > TOOL_ARGS_MAX_BYTES {
                            let sig = ParseErrSig {
                                tool_name: tool_call.name.clone(),
                                error_kind: "too_large".to_string(),
                                args_prefix: args_serialised.chars().take(64).collect(),
                            };
                            let count = self.parse_error_tracker.record(sig);
                            self.counters.consecutive_failures += 1;
                            let err = OrchestratorError::ToolArgsTooLarge {
                                tool_name: tool_call.name.clone(),
                                actual_bytes: args_serialised.len(),
                                limit_bytes: TOOL_ARGS_MAX_BYTES,
                            };
                            warn!("{} (consecutive: {})", err, count);
                            if count >= REPEATED_PARSE_ERROR_LIMIT {
                                let abort = OrchestratorError::RepeatedInvalidToolArgs {
                                    model: self.provider.name().to_string(),
                                    tool_name: tool_call.name.clone(),
                                    count,
                                    input_excerpt: args_serialised.chars().take(200).collect(),
                                };
                                return Err(abort.to_string());
                            }
                            let result_msg = LlmMessage::tool_result(
                                &tool_call.id,
                                format!("{{\"error\":\"{}\"}}", err),
                            );
                            self.context.add_message(result_msg);
                            continue;
                        }
                        self.parse_error_tracker.reset();

                        let result = self
                            .execute_tool(&tool_call.name, tool_call.arguments)
                            .await;

                        if !result.success {
                            self.counters.consecutive_failures += 1;
                        } else {
                            self.counters.consecutive_failures = 0;
                        }

                        let result_msg = if let Some(image_data) = result.metadata.get("image_data") {
                            let media_type = result.metadata.get("media_type")
                                .cloned()
                                .unwrap_or_else(|| "image/png".to_string());
                            
                            LlmMessage {
                                role: crate::provider::Role::Tool,
                                content: crate::provider::MessageContent::Parts(vec![
                                    crate::provider::ContentPart::Text { text: serde_json::to_string(&result).unwrap_or_default() },
                                    crate::provider::ContentPart::Image {
                                        source: crate::provider::ImageSource {
                                            type_: "base64".to_string(),
                                            media_type,
                                            data: image_data.clone(),
                                        }
                                    }
                                ]),
                                tool_call_id: Some(tool_call.id.clone()),
                                tool_calls: None,
                            }
                        } else {
                            let result_json = serde_json::to_string(&result).unwrap_or_default();
                            LlmMessage::tool_result(&tool_call.id, result_json)
                        };
                        self.context.add_message(result_msg);
                    }
                }
                FinishReason::Length => {
                    return Err("Response truncated (max tokens)".to_string());
                }
                FinishReason::Error => {
                    self.counters.consecutive_failures += 1;
                    return Err("LLM returned error".to_string());
                }
            }
        }
    }

    // ─── Full Mission Execution ────────────────────────────────────

    /// Execute a full mission: Plan → (Worker + Critic) loop
    pub async fn execute_mission(&mut self, goal: &str) -> Result<String, String> {
        info!("Starting mission: {}", goal);

        // Phase 1: Plan
        let mission = self.plan(goal).await?;
        let mut results = Vec::new();

        // Phase 2: Execute each step with Worker + review with Critic
        for (i, step) in mission.steps.iter().enumerate() {
            info!(
                "Mission step {}/{}: {}",
                i + 1,
                mission.steps.len(),
                step.description
            );

            if let Some(ref mut m) = self.current_mission {
                if i < m.steps.len() {
                    m.steps[i].status = StepStatus::InProgress;
                }
            }

            let mut retry_count = 0;
            let max_retries = 2;
            let mut current_prompt = step.description.clone();
            let step_output = loop {
                let output = match self.process(&current_prompt).await {
                    Ok(output) => output,
                    Err(e) => {
                        if let Some(ref mut m) = self.current_mission {
                            if i < m.steps.len() {
                                m.steps[i].status = StepStatus::Failed(e.clone());
                            }
                        }
                        return Err(format!("Mission failed at step {}: {}", i + 1, e));
                    }
                };

                let critique = self.critique(&step.description, &output).await?;
                if critique.approved {
                    break output;
                }

                if retry_count >= max_retries {
                    warn!("Critic rejected step {} after {} retries. Continuing.", i + 1, retry_count);
                    break output;
                }

                retry_count += 1;
                info!("Self-Healing: Retrying step {} (attempt {}) with feedback.", i + 1, retry_count + 1);
                current_prompt = format!(
                    "Your previous response for the task '{}' was REJECTED by the critic.\nFeedback: {}\n\nPlease try again, addressing the feedback above.",
                    step.description, critique.feedback
                );
                self.context.clear(); // Fresh start for retry
            };

            if let Some(ref mut m) = self.current_mission {
                if i < m.steps.len() {
                    m.steps[i].status = StepStatus::Completed;
                }
            }

            results.push(format!(
                "Step {}: {}\n{}",
                i + 1,
                step.description,
                step_output
            ));

            self.context.clear();
        }

        let summary = results.join("\n---\n");
        info!("Mission completed: {} steps executed", mission.steps.len());
        Ok(summary)
    }

    pub fn reset(&mut self) {
        self.context.clear();
        self.counters = BudgetCounters::default();
        self.current_mission = None;
        self.parse_error_tracker = ParseErrorTracker::default();
    }
}

// ─── Critic Result ─────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CriticResult {
    pub approved: bool,
    pub feedback: String,
    #[serde(default)]
    pub suggestions: Vec<String>,
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── AgentBudget ─────────────────────────────────────────────
    #[test]
    fn test_budget_default() {
        let budget = AgentBudget::default();
        assert_eq!(budget.max_steps, 50);
        assert_eq!(budget.max_tool_calls, 30);
        assert_eq!(budget.max_llm_calls, 100);
        assert_eq!(budget.max_files_modified, 20);
        assert_eq!(budget.max_duration_minutes, 30);
        assert_eq!(budget.max_consecutive_failures, 3);
    }

    #[test]
    fn test_budget_custom() {
        let budget = AgentBudget {
            max_steps: 10,
            max_tool_calls: 5,
            max_llm_calls: 20,
            max_files_modified: 3,
            max_duration_minutes: 5,
            max_consecutive_failures: 1,
        };
        assert_eq!(budget.max_steps, 10);
        assert_eq!(budget.max_consecutive_failures, 1);
    }

    #[test]
    fn test_budget_clone() {
        let b = AgentBudget::default();
        let b2 = b.clone();
        assert_eq!(b.max_steps, b2.max_steps);
    }

    // ─── Mission ─────────────────────────────────────────────────
    #[test]
    fn test_mission_new() {
        let m = Mission::new("Test goal");
        assert_eq!(m.goal, "Test goal");
        assert!(m.steps.is_empty());
    }

    #[test]
    fn test_mission_new_from_string() {
        let goal = String::from("Dynamic goal");
        let m = Mission::new(goal);
        assert_eq!(m.goal, "Dynamic goal");
    }

    #[test]
    fn test_mission_progress_empty() {
        let m = Mission::new("Empty");
        assert_eq!(m.progress(), (0, 0));
    }

    #[test]
    fn test_mission_progress_partial() {
        let mut m = Mission::new("Test");
        m.steps.push(MissionStep {
            description: "Step 1".to_string(),
            status: StepStatus::Completed,
        });
        m.steps.push(MissionStep {
            description: "Step 2".to_string(),
            status: StepStatus::Pending,
        });
        m.steps.push(MissionStep {
            description: "Step 3".to_string(),
            status: StepStatus::InProgress,
        });
        assert_eq!(m.progress(), (1, 3));
    }

    #[test]
    fn test_mission_progress_all_done() {
        let mut m = Mission::new("Test");
        for i in 0..5 {
            m.steps.push(MissionStep {
                description: format!("Step {}", i),
                status: StepStatus::Completed,
            });
        }
        assert_eq!(m.progress(), (5, 5));
    }

    #[test]
    fn test_mission_clone() {
        let mut m = Mission::new("Original");
        m.steps.push(MissionStep {
            description: "Step 1".to_string(),
            status: StepStatus::Pending,
        });
        let m2 = m.clone();
        assert_eq!(m.goal, m2.goal);
        assert_eq!(m.steps.len(), m2.steps.len());
    }

    // ─── StepStatus ──────────────────────────────────────────────
    #[test]
    fn test_step_status_eq() {
        assert_eq!(StepStatus::Pending, StepStatus::Pending);
        assert_eq!(StepStatus::InProgress, StepStatus::InProgress);
        assert_eq!(StepStatus::Completed, StepStatus::Completed);
        assert_ne!(StepStatus::Pending, StepStatus::Completed);
    }

    #[test]
    fn test_step_status_failed() {
        let s = StepStatus::Failed("oops".to_string());
        if let StepStatus::Failed(msg) = s {
            assert_eq!(msg, "oops");
        } else {
            panic!("Expected Failed");
        }
    }

    #[test]
    fn test_step_status_debug() {
        let s = format!("{:?}", StepStatus::Pending);
        assert!(s.contains("Pending"));
    }

    // ─── MissionStep ─────────────────────────────────────────────
    #[test]
    fn test_mission_step() {
        let step = MissionStep {
            description: "Read the file".to_string(),
            status: StepStatus::Pending,
        };
        assert_eq!(step.description, "Read the file");
        assert_eq!(step.status, StepStatus::Pending);
    }

    #[test]
    fn test_mission_step_clone() {
        let step = MissionStep {
            description: "Clone me".to_string(),
            status: StepStatus::InProgress,
        };
        let step2 = step.clone();
        assert_eq!(step.description, step2.description);
    }

    // ─── CriticResult ────────────────────────────────────────────
    #[test]
    fn test_critic_result_approved() {
        let json = r#"{"approved": true, "feedback": "Looks good"}"#;
        let r: CriticResult = serde_json::from_str(json).unwrap();
        assert!(r.approved);
        assert_eq!(r.feedback, "Looks good");
        assert!(r.suggestions.is_empty());
    }

    #[test]
    fn test_critic_result_rejected() {
        let json = r#"{"approved": false, "feedback": "Bad code", "suggestions": ["Fix X", "Add tests"]}"#;
        let r: CriticResult = serde_json::from_str(json).unwrap();
        assert!(!r.approved);
        assert_eq!(r.suggestions.len(), 2);
    }

    #[test]
    fn test_critic_result_without_suggestions() {
        let json = r#"{"approved": true, "feedback": "OK"}"#;
        let r: CriticResult = serde_json::from_str(json).unwrap();
        assert!(r.suggestions.is_empty());
    }

    // ─── BudgetCounters ──────────────────────────────────────────
    #[test]
    fn test_budget_counters_default() {
        let c = BudgetCounters::default();
        assert_eq!(c.steps, 0);
        assert_eq!(c.tool_calls, 0);
        assert_eq!(c.llm_calls, 0);
        assert_eq!(c.consecutive_failures, 0);
    }

    // ─── Stress Tests ────────────────────────────────────────────
    #[test]
    fn stress_test_mission_many_steps() {
        let mut m = Mission::new("Large mission");
        for i in 0..1000 {
            m.steps.push(MissionStep {
                description: format!("Step {}", i),
                status: if i % 3 == 0 {
                    StepStatus::Completed
                } else {
                    StepStatus::Pending
                },
            });
        }
        let (done, total) = m.progress();
        assert_eq!(total, 1000);
        assert!(done > 300); // ~334 should be completed
    }

    #[test]
    fn stress_test_critic_result_list() {
        for i in 0..500 {
            let json = format!(
                r#"{{"approved": {}, "feedback": "feedback_{}", "suggestions": []}}"#,
                if i % 2 == 0 { "true" } else { "false" },
                i
            );
            let r: CriticResult = serde_json::from_str(&json).unwrap();
            assert_eq!(r.feedback, format!("feedback_{}", i));
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // C1 — Unit tests: validate_and_parse_tool_args + ParseErrorTracker
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn tool_args_valid_json_parses_ok() {
        let v = validate_and_parse_tool_args("exec", r#"{"cmd":"echo","args":["hello"]}"#);
        assert!(v.is_ok());
        assert_eq!(v.unwrap()["cmd"], "echo");
    }

    #[test]
    fn tool_args_truncated_json_returns_err() {
        let truncated = r#"{"cmd":"echo","#;
        let err = validate_and_parse_tool_args("exec", truncated).unwrap_err();
        assert!(
            matches!(err, OrchestratorError::RepeatedInvalidToolArgs { .. }),
            "Expected RepeatedInvalidToolArgs"
        );
        if let OrchestratorError::RepeatedInvalidToolArgs { tool_name, .. } = err {
            assert_eq!(tool_name, "exec");
        }
    }

    #[test]
    fn tool_args_trailing_chars_returns_err() {
        let bad = r#"{"cmd":"echo"}trailing garbage"#;
        let err = validate_and_parse_tool_args("exec", bad).unwrap_err();
        assert!(matches!(err, OrchestratorError::RepeatedInvalidToolArgs { .. }));
    }

    #[test]
    fn tool_args_empty_string_returns_err() {
        let err = validate_and_parse_tool_args("exec", "").unwrap_err();
        assert!(matches!(err, OrchestratorError::RepeatedInvalidToolArgs { .. }));
    }

    #[test]
    fn tool_args_over_size_limit_returns_too_large() {
        let big = "x".repeat(TOOL_ARGS_MAX_BYTES + 1);
        let err = validate_and_parse_tool_args("exec", &big).unwrap_err();
        assert!(
            matches!(err, OrchestratorError::ToolArgsTooLarge { .. }),
            "Expected ToolArgsTooLarge"
        );
        if let OrchestratorError::ToolArgsTooLarge { actual_bytes, limit_bytes, .. } = err {
            assert!(actual_bytes > limit_bytes);
            assert_eq!(limit_bytes, TOOL_ARGS_MAX_BYTES);
        }
    }

    #[test]
    fn tool_args_16kb_plus_one_triggers_too_large() {
        let oversized = "a".repeat(16 * 1024 + 1);
        let err = validate_and_parse_tool_args("my_tool", &oversized).unwrap_err();
        assert!(matches!(err, OrchestratorError::ToolArgsTooLarge { .. }));
    }

    #[test]
    fn parse_error_tracker_increments_same_sig() {
        let mut tracker = ParseErrorTracker::default();
        let sig = ParseErrSig {
            tool_name: "exec".to_string(),
            error_kind: "unexpected_eof".to_string(),
            args_prefix: r#"{"cmd":"echo","#.to_string(),
        };
        assert_eq!(tracker.record(sig.clone()), 1);
        assert_eq!(tracker.record(sig.clone()), 2);
        assert_eq!(tracker.record(sig.clone()), 3);
    }

    #[test]
    fn parse_error_tracker_resets_on_different_sig() {
        let mut tracker = ParseErrorTracker::default();
        let sig_a = ParseErrSig {
            tool_name: "exec".to_string(),
            error_kind: "unexpected_eof".to_string(),
            args_prefix: "aaa".to_string(),
        };
        let sig_b = ParseErrSig {
            tool_name: "exec".to_string(),
            error_kind: "syntax_error".to_string(),
            args_prefix: "bbb".to_string(),
        };
        tracker.record(sig_a.clone());
        tracker.record(sig_a);
        let count = tracker.record(sig_b);
        assert_eq!(count, 1, "Different signature should reset count to 1");
    }

    #[test]
    fn parse_error_tracker_reset_clears() {
        let mut tracker = ParseErrorTracker::default();
        let sig = ParseErrSig {
            tool_name: "t".to_string(),
            error_kind: "e".to_string(),
            args_prefix: "p".to_string(),
        };
        tracker.record(sig.clone());
        tracker.record(sig.clone());
        tracker.reset();
        assert_eq!(tracker.record(sig), 1);
    }

    #[test]
    fn parse_tracker_reaches_limit_exactly() {
        let mut tracker = ParseErrorTracker::default();
        let sig = ParseErrSig {
            tool_name: "exec".to_string(),
            error_kind: "unexpected_eof".to_string(),
            args_prefix: r#"{"cmd":"#.to_string(),
        };
        for i in 1..=REPEATED_PARSE_ERROR_LIMIT {
            let count = tracker.record(sig.clone());
            assert_eq!(count, i);
        }
        assert!(tracker.count >= REPEATED_PARSE_ERROR_LIMIT);
    }

    #[test]
    fn orchestrator_error_repeated_display_contains_key_info() {
        let err = OrchestratorError::RepeatedInvalidToolArgs {
            model: "llama-3.3-70b".to_string(),
            tool_name: "exec".to_string(),
            count: 3,
            input_excerpt: r#"{"cmd":"echo","#.to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("exec"));
        assert!(msg.contains("3"));
        assert!(msg.contains("llama-3.3-70b"));
        assert!(msg.contains("switch model"));
        assert!(!msg.contains('\n'), "Error message should be single-line safe");
    }

    #[test]
    fn orchestrator_error_too_large_display_contains_sizes() {
        let err = OrchestratorError::ToolArgsTooLarge {
            tool_name: "read_file".to_string(),
            actual_bytes: 20_000,
            limit_bytes: TOOL_ARGS_MAX_BYTES,
        };
        let msg = err.to_string();
        assert!(msg.contains("read_file"));
        assert!(msg.contains("20000"));
        assert!(msg.contains("truncated"));
    }

    #[test]
    fn fixture_stream_error_message_is_stable() {
        // C2: fixture — error message must be stable (no timestamps/addresses)
        let err = OrchestratorError::RepeatedInvalidToolArgs {
            model: "meta-llama/llama-3.3-70b-instruct".to_string(),
            tool_name: "exec".to_string(),
            count: REPEATED_PARSE_ERROR_LIMIT,
            input_excerpt: r#"{"cmd":"echo","args":["test"]"#.chars().take(200).collect(),
        };
        let msg = err.to_string();
        assert!(msg.contains("exec"));
        assert!(msg.contains("meta-llama"));
        assert!(msg.contains(&REPEATED_PARSE_ERROR_LIMIT.to_string()));
        assert!(msg.contains("switch model"));
        assert!(msg.contains("reduce tool payload"));
        // No time-dependent content
        assert!(!msg.contains('\n'));
    }

    // ═══════════════════════════════════════════════════════════════
    // C2 — Integration: mock provider with oversized tool args
    //      Verifies orchestrator aborts (no infinite loop)
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn orchestrator_aborts_on_repeated_large_tool_args() {
        use async_trait::async_trait;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        struct BrokenToolProvider {
            call_count: Arc<AtomicU32>,
        }

        #[async_trait]
        impl crate::provider::LlmProvider for BrokenToolProvider {
            async fn complete(
                &self,
                _messages: Vec<crate::provider::LlmMessage>,
                _tools: Option<Vec<crate::provider::ToolDefinition>>,
                _model_override: Option<String>,
            ) -> Result<crate::provider::LlmResponse, crate::provider::ProviderError> {
                self.call_count.fetch_add(1, Ordering::SeqCst);
                // Always return oversized tool args
                Ok(crate::provider::LlmResponse {
                    content: None,
                    tool_calls: vec![crate::provider::ToolCall {
                        id: "call_bad".to_string(),
                        name: "exec".to_string(),
                        arguments: serde_json::json!({
                            "cmd": "x".repeat(TOOL_ARGS_MAX_BYTES + 100)
                        }),
                    }],
                    finish_reason: crate::provider::FinishReason::ToolCalls,
                    usage: None,
                })
            }

            fn name(&self) -> &str {
                "broken-llama-mock"
            }
        }

        let call_counter = Arc::new(AtomicU32::new(0));
        let provider = Arc::new(BrokenToolProvider {
            call_count: Arc::clone(&call_counter),
        });

        let tmp = tempfile::tempdir().unwrap();
        let mut orc = AgentOrchestrator::new(provider, tmp.path().to_path_buf(), 8192);
        // Override budget: let anti-loop guard fire, not consecutive_failures
        orc.set_budget(AgentBudget {
            max_steps: 200,
            max_tool_calls: 200,
            max_llm_calls: 200,
            max_files_modified: 20,
            max_duration_minutes: 10,
            max_consecutive_failures: 200,
        });

        let result = orc.process("do something").await;
        assert!(result.is_err(), "Should abort on repeated large args");
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("too large") || err_msg.contains("Orchestrator aborted"),
            "Expected size/anti-loop error, got: {}",
            err_msg
        );

        // The loop MUST terminate within LIMIT + small overhead (not run forever)
        let calls = call_counter.load(Ordering::SeqCst);
        assert!(
            calls <= REPEATED_PARSE_ERROR_LIMIT + 3,
            "Too many provider calls ({}) — anti-loop guard not working",
            calls
        );
    }

    #[tokio::test]
    async fn orchestrator_normal_flow_not_affected_by_guard() {
        use async_trait::async_trait;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        struct GoodProvider {
            calls: Arc<AtomicU32>,
        }

        #[async_trait]
        impl crate::provider::LlmProvider for GoodProvider {
            async fn complete(
                &self,
                _messages: Vec<crate::provider::LlmMessage>,
                _tools: Option<Vec<crate::provider::ToolDefinition>>,
                _model_override: Option<String>,
            ) -> Result<crate::provider::LlmResponse, crate::provider::ProviderError> {
                let n = self.calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(crate::provider::LlmResponse {
                        content: None,
                        tool_calls: vec![crate::provider::ToolCall {
                            id: "call_valid".to_string(),
                            name: "echo".to_string(),
                            arguments: serde_json::json!({"text": "hello"}),
                        }],
                        finish_reason: crate::provider::FinishReason::ToolCalls,
                        usage: None,
                    })
                } else {
                    Ok(crate::provider::LlmResponse {
                        content: Some("done".to_string()),
                        tool_calls: vec![],
                        finish_reason: crate::provider::FinishReason::Stop,
                        usage: None,
                    })
                }
            }

            fn name(&self) -> &str {
                "good-mock"
            }
        }

        let provider = Arc::new(GoodProvider { calls: Arc::new(AtomicU32::new(0)) });
        let tmp = tempfile::tempdir().unwrap();
        let mut orc = AgentOrchestrator::new(provider, tmp.path().to_path_buf(), 8192);
        let result = orc.process("do something valid").await;
        // Should return Ok or a tool-execution failure — NOT an anti-loop error
        if let Err(ref e) = result {
            assert!(
                !e.contains("Orchestrator aborted"),
                "Should not trigger anti-loop for valid args, got: {}",
                e
            );
        }
    }
}
