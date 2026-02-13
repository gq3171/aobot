mod chat;
mod send;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aobot", about = "AI Agent Gateway CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive chat session with an AI agent
    Chat {
        /// Model ID to use (e.g. "anthropic/claude-sonnet-4")
        #[arg(short, long)]
        model: Option<String>,

        /// System prompt override
        #[arg(short, long)]
        system_prompt: Option<String>,

        /// Working directory for tools
        #[arg(short, long)]
        working_dir: Option<String>,
    },
    /// Start the Gateway WebSocket server
    Gateway {
        /// Port to listen on (overrides config)
        #[arg(short, long)]
        port: Option<u16>,

        /// Working directory for agent tools
        #[arg(short, long)]
        working_dir: Option<String>,
    },
    /// Send a message to an agent via the Gateway
    Send {
        /// Message to send
        #[arg(short, long)]
        message: String,

        /// Gateway WebSocket URL
        #[arg(long, default_value = "ws://127.0.0.1:3000/ws")]
        url: String,

        /// Session key (auto-generated if not provided)
        #[arg(long)]
        session_key: Option<String>,

        /// Agent name to use
        #[arg(long)]
        agent: Option<String>,

        /// Bearer token for authentication
        #[arg(long)]
        token: Option<String>,
    },
    /// Check system health
    Health,
}

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Chat {
            model,
            system_prompt,
            working_dir,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(chat::run_chat(model, system_prompt, working_dir))?;
        }
        Commands::Gateway { port, working_dir } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                let config = aobot_config::load_config().unwrap_or_default();
                let wd = match working_dir {
                    Some(dir) => std::path::PathBuf::from(dir),
                    None => std::env::current_dir()?,
                };

                let mut channel_factories: std::collections::HashMap<
                    String,
                    aobot_gateway::ChannelFactory,
                > = std::collections::HashMap::new();
                channel_factories.insert(
                    "telegram".into(),
                    Box::new(aobot_channel_telegram::create_telegram_channel),
                );
                channel_factories.insert(
                    "discord".into(),
                    Box::new(aobot_channel_discord::create_discord_channel),
                );

                aobot_gateway::start_gateway(config, wd, port, channel_factories)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })?;
        }
        Commands::Send {
            message,
            url,
            session_key,
            agent,
            token,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(send::run_send(message, url, session_key, agent, token))?;
        }
        Commands::Health => {
            println!("aobot is healthy");
            let config = aobot_config::load_config().unwrap_or_default();
            println!("  default agent: {}", config.default_agent);
            println!("  agents configured: {}", config.agents.len());
            println!("  gateway port: {}", config.gateway.port);
        }
    }

    Ok(())
}
