use std::io::{self, BufRead, Write};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;

use pi_agent_ai::register::create_default_registry;
use pi_agent_ai::stream::stream_simple;
use pi_agent_core::agent_types::{AgentEvent, StreamFnBox};
use pi_agent_core::event_stream::create_assistant_message_event_stream;
use pi_agent_core::types::*;
use pi_coding_agent::agent_session::events::AgentSessionEvent;
use pi_coding_agent::agent_session::sdk::{CreateSessionOptions, create_agent_session};
use pi_coding_agent::agent_session::session::PromptOptions;
use pi_coding_agent::tools::create_coding_tools;

/// Run the interactive chat REPL.
pub async fn run_chat(
    model_id: Option<String>,
    system_prompt: Option<String>,
    working_dir_override: Option<String>,
) -> Result<()> {
    let config = aobot_config::load_config().unwrap_or_default();

    // Determine working directory
    let working_dir = match working_dir_override {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::env::current_dir().context("Failed to get current directory")?,
    };

    // Determine model ID: CLI flag > config > default
    let effective_model = model_id
        .or_else(|| {
            config
                .agents
                .get(&config.default_agent)
                .map(|a| a.model.clone())
        })
        .unwrap_or_else(|| "anthropic/claude-sonnet-4".to_string());

    // Create agent session
    let mut session = create_agent_session(CreateSessionOptions {
        working_dir: working_dir.clone(),
        model_id: Some(effective_model.clone()),
        ..Default::default()
    })
    .map_err(|e| anyhow::anyhow!("Failed to create agent session: {e}"))?;

    // Set up API registry and stream function
    let registry = Arc::new(create_default_registry());

    let stream_fn: StreamFnBox = Arc::new(move |model, context, options| {
        let cancel = CancellationToken::new();
        match stream_simple(model, context, options, &registry, cancel) {
            Ok(stream) => stream,
            Err(err) => {
                let stream = create_assistant_message_event_stream();
                let mut msg = AssistantMessage::empty(model);
                msg.stop_reason = StopReason::Error;
                msg.error_message = Some(err);
                stream.push(AssistantMessageEvent::Error {
                    reason: StopReason::Error,
                    error: msg,
                });
                stream
            }
        }
    });

    session.set_stream_fn(stream_fn);

    // Set up tools
    let tools = create_coding_tools(&working_dir);
    session.set_tools(tools);

    // Set system prompt
    let prompt = system_prompt
        .or_else(|| {
            config
                .agents
                .get(&config.default_agent)
                .and_then(|a| a.system_prompt.clone())
        })
        .unwrap_or_else(|| "You are a helpful assistant.".to_string());
    session.set_system_prompt(prompt);

    // Subscribe to events for streaming output
    session.subscribe(Box::new(|event| match &event {
        AgentSessionEvent::Agent(AgentEvent::MessageUpdate {
            assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
            ..
        }) => {
            print!("{delta}");
            let _ = io::stdout().flush();
        }
        AgentSessionEvent::Agent(AgentEvent::ToolExecutionStart { tool_name, .. }) => {
            eprintln!("\n[tool: {tool_name}]");
        }
        AgentSessionEvent::Agent(AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            ..
        }) => {
            if *is_error {
                eprintln!("[tool: {tool_name} - error]");
            }
        }
        AgentSessionEvent::Error { message } => {
            eprintln!("\n[error: {message}]");
        }
        _ => {}
    }));

    // Print welcome message
    let model_display = session
        .model()
        .map(|m| m.id.as_str())
        .unwrap_or(&effective_model);
    println!("aobot chat (model: {model_display})");
    println!("Type your message and press Enter. Type 'exit' or Ctrl+D to quit.\n");

    // Interactive loop
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut line = String::new();
        let bytes = stdin.lock().read_line(&mut line)?;
        if bytes == 0 {
            // EOF (Ctrl+D)
            println!();
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == "exit" || input == "quit" {
            break;
        }

        // Send prompt
        match session.prompt(input, PromptOptions::default()).await {
            Ok(()) => {
                // Ensure newline after streaming output
                println!();
            }
            Err(e) => {
                eprintln!("\n[prompt error: {e}]");
            }
        }
    }

    println!("Goodbye!");
    Ok(())
}
