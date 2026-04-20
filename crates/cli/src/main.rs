mod client;
mod cmd;
mod config;
mod render;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::client::Client;

#[derive(Parser, Debug)]
#[command(name = "joi", about = "Joi multi-actor collaboration CLI")]
struct Args {
    /// Override the configured server URL (defaults to ws://127.0.0.1:7878/rpc).
    #[arg(long, global = true, env = "JOI_SERVER")]
    server: Option<String>,
    /// Override the configured local actor id.
    #[arg(long = "as", global = true, env = "JOI_ACTOR")]
    actor: Option<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Show local config + server info.
    Who,
    /// Manage spaces.
    Space {
        #[command(subcommand)]
        sub: SpaceCmd,
    },
    /// Manage conversations.
    Conv {
        #[command(subcommand)]
        sub: ConvCmd,
    },
    /// Send a content.add event into a conversation.
    Say {
        text: String,
        #[arg(long)]
        r#in: String,
        #[arg(long)]
        reply: Option<String>,
    },
    /// Send a handoff.offer to an agent in a conversation.
    Handoff {
        /// Target actor id; omit to pick from a list of registered agents/humans.
        agent: Option<String>,
        #[arg(long)]
        r#in: String,
        #[arg(long, default_value = "")]
        message: String,
    },
    /// Respond to an action.request event.
    Action {
        #[command(subcommand)]
        sub: ActionCmd,
    },
    /// Manage agents.
    Agent {
        #[command(subcommand)]
        sub: AgentCmd,
    },
    /// Interactive chat REPL inside a conversation.
    Chat {
        #[arg(long)]
        r#in: String,
    },
}

#[derive(Subcommand, Debug)]
enum SpaceCmd {
    Create {
        #[arg(long)]
        title: String,
    },
    List,
}

#[derive(Subcommand, Debug)]
enum ConvCmd {
    Create {
        #[arg(long)]
        space: String,
        #[arg(long, default_value = "Untitled")]
        title: String,
    },
    List {
        #[arg(long)]
        space: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ActionCmd {
    Accept {
        event_id: String,
        #[arg(long, default_value = "allow")]
        option: String,
    },
    Decline {
        event_id: String,
        #[arg(long, default_value = "deny")]
        option: String,
    },
}

#[derive(Subcommand, Debug)]
enum AgentCmd {
    /// List locally registered agents.
    List,
    /// Show the bundled marketplace catalog.
    Marketplace,
    /// Install an agent from the bundled marketplace.
    Install {
        marketplace_id: String,
        /// Local actor id to assign (defaults to `actor_<marketplace_id>`).
        #[arg(long = "actor-id")]
        local_actor_id: Option<String>,
        #[arg(long = "name")]
        display_name: Option<String>,
        /// Force a specific distribution: auto (default), npx, uvx, binary.
        #[arg(long)]
        prefer: Option<String>,
    },
    /// Add a custom agent interactively.
    Add,
    /// Register an agent from a local JSON spec file.
    Register {
        path: PathBuf,
    },
    /// Remove a locally registered agent.
    Remove {
        actor_id: String,
    },
    Start {
        actor_id: String,
    },
    Stop {
        actor_id: String,
    },
    Log {
        actor_id: String,
        #[arg(long, default_value_t = 50)]
        tail: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    let cfg = config::resolve(args.server.clone(), args.actor.clone())?;

    if let Cmd::Who = args.cmd {
        println!("server   = {}", cfg.server_url);
        println!("actor    = {}", cfg.actor_id);
        println!("display  = {}", cfg.display_name);
        println!("config   = {}", config::config_path().display());
        return Ok(());
    }

    let client = Client::connect(&cfg.server_url).await?;
    client.initialize().await?;
    let _ = client
        .open_connection(&cfg.actor_id, Some(&cfg.display_name))
        .await?;

    match args.cmd {
        Cmd::Who => unreachable!(),
        Cmd::Space { sub } => match sub {
            SpaceCmd::Create { title } => cmd::space::create(client, title).await?,
            SpaceCmd::List => cmd::space::list(client).await?,
        },
        Cmd::Conv { sub } => match sub {
            ConvCmd::Create { space, title } => cmd::conv::create(client, space, title).await?,
            ConvCmd::List { space } => cmd::conv::list(client, space).await?,
        },
        Cmd::Say { text, r#in, reply } => {
            cmd::say::run(client, cfg.actor_id, r#in, text, reply).await?
        }
        Cmd::Handoff {
            agent,
            r#in,
            message,
        } => cmd::handoff::run(client, cfg.actor_id, agent, r#in, message).await?,
        Cmd::Action { sub } => match sub {
            ActionCmd::Accept { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, true).await?
            }
            ActionCmd::Decline { event_id, option } => {
                cmd::action::respond(client, cfg.actor_id, event_id, option, false).await?
            }
        },
        Cmd::Agent { sub } => match sub {
            AgentCmd::List => cmd::agent::list(client).await?,
            AgentCmd::Marketplace => cmd::agent::marketplace(client).await?,
            AgentCmd::Install {
                marketplace_id,
                local_actor_id,
                display_name,
                prefer,
            } => {
                cmd::agent::install(client, marketplace_id, local_actor_id, display_name, prefer)
                    .await?
            }
            AgentCmd::Add => cmd::agent::add(client).await?,
            AgentCmd::Register { path } => cmd::agent::register(client, path).await?,
            AgentCmd::Remove { actor_id } => cmd::agent::remove(client, actor_id).await?,
            AgentCmd::Start { actor_id } => cmd::agent::start(client, actor_id).await?,
            AgentCmd::Stop { actor_id } => cmd::agent::stop(client, actor_id).await?,
            AgentCmd::Log { actor_id, tail } => cmd::agent::log(client, actor_id, tail).await?,
        },
        Cmd::Chat { r#in } => cmd::chat::run(client, cfg.actor_id, r#in).await?,
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();
}
