pub mod agent;
pub mod config;
pub mod context;
pub mod edit_plan;
pub mod providers;
pub mod rpc;
pub mod session;
pub mod tool_parser;
pub mod tools;

// Re-export commonly used types for integration tests
pub use agent::{AgentResponse, CodingAgent};
pub use config::Config;
pub use context::{build_native_tool_defs, build_project_context, build_tools_prompt, get_reasoning_prompt, get_system_prompt};
pub use edit_plan::{generate_edit_plan, run_validation_commands, ValidationRunResult};
pub use providers::{AiProvider, ChatMessage, ChatRequest, ChatResponse, ChatStreamChunk, NativeFunctionCall, NativeToolCall, Role, StreamCallback};
pub use rpc::{map_reasoning_effort, RpcHandler};
pub use session::{AgentSession, SessionInfo, SessionSnapshot};
pub use tool_parser::{extract_tool_calls_from, ToolCallRequest};
pub use tools::{ToolDefinition, ToolEvent, ToolEventSink, ToolName, ToolRegistry, ToolResult};
