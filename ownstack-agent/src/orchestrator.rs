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
}

impl AgentOrchestrator {
    fn encode_tool_name(namespace: &str, local_name: &str) -> String {
        format!("{}__{}", namespace, local_name)
    }

    fn normalize_tool_name(toolkits: &[Arc<dyn Toolkit>], raw_name: &str) -> String {
        if raw_name.contains(':') {
            return raw_name.to_string();
        }

        if let Some((namespace, local_name)) = raw_name.split_once("__") {
            if toolkits.iter().any(|tk| tk.name() == namespace) {
                return format!("{}:{}", namespace, local_name);
            }
        }

        raw_name.to_string()
    }

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
            .flat_map(|tk| {
                let toolkit_name = tk.name().to_string();
                tk.tools().into_iter().map(move |t| {
                    let canonical = format!("{}:{}", toolkit_name, t.name);
                    let provider_safe =
                        Self::encode_tool_name(&toolkit_name, &t.name);
                    ToolDefinition {
                        name: provider_safe,
                        description: format!(
                            "{} (canonical tool id: {})",
                            t.description, canonical
                        ),
                        parameters: t.parameters,
                    }
                })
            })
            .collect()
    }

    pub async fn execute_tool_shared(
        toolkits: &[Arc<dyn Toolkit>],
        name: &str,
        args: serde_json::Value,
    ) -> ToolResult {
        let normalized_name = Self::normalize_tool_name(toolkits, name);

        if let Some((namespace, local_name)) = normalized_name.split_once(':') {
            for toolkit in toolkits {
                if toolkit.name() != namespace {
                    continue;
                }

                return match toolkit.execute(local_name, args).await {
                    Ok(result) => result,
                    Err(e) => ToolResult::failure(format!("{}", e), None),
                };
            }

            return ToolResult::failure(
                format!(
                    "Toolkit not found for namespaced tool: {}",
                    normalized_name
                ),
                None,
            );
        }

        // Backward-compatibility mode for legacy aliases (single-cycle migration).
        let mut alias_matches = Vec::new();
        for toolkit in toolkits {
            if toolkit
                .tools()
                .iter()
                .any(|def| def.name == normalized_name)
            {
                alias_matches.push(toolkit.clone());
            }
        }

        if alias_matches.len() > 1 {
            let suggestions = alias_matches
                .iter()
                .map(|tk| format!("{}:{}", tk.name(), name))
                .collect::<Vec<_>>()
                .join(", ");
            return ToolResult::failure(
                format!(
                    "Ambiguous legacy tool alias '{}'. Use namespaced tool id: {}",
                    normalized_name, suggestions
                ),
                None,
            );
        }

        if let Some(toolkit) = alias_matches.into_iter().next() {
            return match toolkit.execute(&normalized_name, args).await {
                Ok(result) => result,
                Err(e) => ToolResult::failure(format!("{}", e), None),
            };
        }

        for toolkit in toolkits {
            match toolkit.execute(&normalized_name, args.clone()).await {
                Ok(result) => return result,
                Err(e) => match e {
                    ToolkitError::ToolNotFound(_) => continue,
                    _ => return ToolResult::failure(format!("{}", e), None),
                },
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
            ("anthropic", "fast") => Some("claude-haiku-4-5-20251001".to_string()),
            ("openrouter", "planning") => {
                Some("anthropic/claude-sonnet-4-6".to_string())
            }
            ("openrouter", "fast") => {
                Some("anthropic/claude-haiku-4-5-20251001".to_string())
            }
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
            let augmented =
                format!("{}\n\n{}\n\n{}", base_prompt, project_rules, rag_context);
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
                        let arguments: serde_json::Value =
                            serde_json::from_str(args_str)
                                .unwrap_or(serde_json::Value::Null);

                        tool_calls.push(crate::provider::ToolCall {
                            id: id
                                .clone()
                                .unwrap_or_else(|| format!("call_{}", idx)),
                            name: name.clone().unwrap_or_default(),
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
                            debug!(
                                "Executing (parallel): {} ({})",
                                tool_call.name, tool_call.arguments
                            );
                            let result = Self::execute_tool_shared(
                                &tks,
                                &tool_call.name,
                                tool_call.arguments.clone(),
                            )
                            .await;
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

                        let result_msg = if let Some(image_data) =
                            result.metadata.get("image_data")
                        {
                            let media_type = result
                                .metadata
                                .get("media_type")
                                .cloned()
                                .unwrap_or_else(|| "image/png".to_string());

                            LlmMessage {
                                role: crate::provider::Role::Tool,
                                content: crate::provider::MessageContent::Parts(
                                    vec![
                                        crate::provider::ContentPart::Text {
                                            text: serde_json::to_string(&result)
                                                .unwrap_or_default(),
                                        },
                                        crate::provider::ContentPart::Image {
                                            source: crate::provider::ImageSource {
                                                type_: "base64".to_string(),
                                                media_type,
                                                data: image_data.clone(),
                                            },
                                        },
                                    ],
                                ),
                                tool_call_id: Some(tool_call.id.clone()),
                                tool_calls: None,
                            }
                        } else {
                            let result_json =
                                serde_json::to_string(&result).unwrap_or_default();
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
                        let result = self
                            .execute_tool(&tool_call.name, tool_call.arguments)
                            .await;

                        if !result.success {
                            self.counters.consecutive_failures += 1;
                        } else {
                            self.counters.consecutive_failures = 0;
                        }

                        let result_msg = if let Some(image_data) =
                            result.metadata.get("image_data")
                        {
                            let media_type = result
                                .metadata
                                .get("media_type")
                                .cloned()
                                .unwrap_or_else(|| "image/png".to_string());

                            LlmMessage {
                                role: crate::provider::Role::Tool,
                                content: crate::provider::MessageContent::Parts(
                                    vec![
                                        crate::provider::ContentPart::Text {
                                            text: serde_json::to_string(&result)
                                                .unwrap_or_default(),
                                        },
                                        crate::provider::ContentPart::Image {
                                            source: crate::provider::ImageSource {
                                                type_: "base64".to_string(),
                                                media_type,
                                                data: image_data.clone(),
                                            },
                                        },
                                    ],
                                ),
                                tool_call_id: Some(tool_call.id.clone()),
                                tool_calls: None,
                            }
                        } else {
                            let result_json =
                                serde_json::to_string(&result).unwrap_or_default();
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
                        return Err(format!(
                            "Mission failed at step {}: {}",
                            i + 1,
                            e
                        ));
                    }
                };

                let critique = self.critique(&step.description, &output).await?;
                if critique.approved {
                    break output;
                }

                if retry_count >= max_retries {
                    warn!(
                        "Critic rejected step {} after {} retries. Continuing.",
                        i + 1,
                        retry_count
                    );
                    break output;
                }

                retry_count += 1;
                info!(
                    "Self-Healing: Retrying step {} (attempt {}) with feedback.",
                    i + 1,
                    retry_count + 1
                );
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
    use async_trait::async_trait;
    use std::collections::HashMap;

    struct MockToolkit {
        toolkit_name: String,
        handled_tools: Vec<String>,
    }

    #[async_trait]
    impl Toolkit for MockToolkit {
        fn name(&self) -> &str {
            &self.toolkit_name
        }

        fn tools(&self) -> Vec<crate::toolkits::ToolDef> {
            self.handled_tools
                .iter()
                .map(|tool| crate::toolkits::ToolDef {
                    name: tool.clone(),
                    description: format!("{} tool", tool),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                })
                .collect()
        }

        async fn execute(
            &self,
            tool_name: &str,
            _args: serde_json::Value,
        ) -> Result<ToolResult, ToolkitError> {
            if self.handled_tools.iter().any(|t| t == tool_name) {
                let payload = HashMap::from([
                    ("toolkit".to_string(), self.toolkit_name.clone()),
                    ("tool".to_string(), tool_name.to_string()),
                ]);
                Ok(ToolResult::success(
                    serde_json::to_string(&payload).unwrap_or_default(),
                ))
            } else {
                Err(ToolkitError::ToolNotFound(tool_name.to_string()))
            }
        }
    }

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

    #[tokio::test]
    async fn test_execute_tool_shared_namespaced_dispatch() {
        let toolkits: Vec<Arc<dyn Toolkit>> = vec![
            Arc::new(MockToolkit {
                toolkit_name: "alpha".to_string(),
                handled_tools: vec!["run".to_string()],
            }),
            Arc::new(MockToolkit {
                toolkit_name: "beta".to_string(),
                handled_tools: vec!["run".to_string()],
            }),
        ];

        let result = AgentOrchestrator::execute_tool_shared(
            &toolkits,
            "alpha:run",
            serde_json::json!({}),
        )
        .await;

        assert!(result.success);
        assert!(result.stdout.contains("\"toolkit\":\"alpha\""));
    }

    #[tokio::test]
    async fn test_execute_tool_shared_legacy_alias_ambiguous() {
        let toolkits: Vec<Arc<dyn Toolkit>> = vec![
            Arc::new(MockToolkit {
                toolkit_name: "alpha".to_string(),
                handled_tools: vec!["run".to_string()],
            }),
            Arc::new(MockToolkit {
                toolkit_name: "beta".to_string(),
                handled_tools: vec!["run".to_string()],
            }),
        ];

        let result = AgentOrchestrator::execute_tool_shared(
            &toolkits,
            "run",
            serde_json::json!({}),
        )
        .await;

        assert!(!result.success);
        assert!(result.stderr.contains("Ambiguous legacy tool alias"));
        assert!(result.stderr.contains("alpha:run"));
        assert!(result.stderr.contains("beta:run"));
    }

    #[tokio::test]
    async fn test_execute_tool_shared_legacy_alias_unique() {
        let toolkits: Vec<Arc<dyn Toolkit>> = vec![
            Arc::new(MockToolkit {
                toolkit_name: "core".to_string(),
                handled_tools: vec!["exec".to_string()],
            }),
            Arc::new(MockToolkit {
                toolkit_name: "git".to_string(),
                handled_tools: vec!["git_status".to_string()],
            }),
        ];

        let result = AgentOrchestrator::execute_tool_shared(
            &toolkits,
            "exec",
            serde_json::json!({}),
        )
        .await;

        assert!(result.success);
        assert!(result.stdout.contains("\"toolkit\":\"core\""));
    }
}
