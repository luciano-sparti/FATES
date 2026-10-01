mod cli;
mod commands;
mod config;
mod dashboard;
mod error;
pub mod health;
mod shell;
mod state;

use clap::{CommandFactory, Parser};
use cli::{Cli, Commands};
use std::path::PathBuf;

fn main() {
    let args = Cli::parse();
    let cfg = args.config;
    let state_dir = resolve_state_dir(args.state_dir.as_deref());

    let result = match args.command {
        Commands::Spin { name, cmd } => commands::spin(name, cmd, &cfg, &state_dir),
        Commands::Draw { name, .. } => commands::draw(name, &cfg, &state_dir),
        Commands::Cut { name, force, .. } => commands::cut(name, force, &cfg, &state_dir),
        Commands::Loom { json, watch } => commands::loom(json, watch, &state_dir),
        Commands::Weave { name } => commands::weave(name, &cfg),
        Commands::Omen { name } => commands::omen(name, &state_dir),
        Commands::Logs {
            name,
            all,
            tail,
            follow,
        } => commands::logs(name, all, tail, follow, &state_dir),
        Commands::Wait { name, timeout } => commands::wait(name, timeout, &state_dir),
        Commands::Init { path } => commands::init(&path),
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "fates", &mut std::io::stdout());
            Ok(())
        }
        Commands::Reel { name, keep } => commands::reel(name, keep, &state_dir),
        Commands::Respin { name, force, .. } => commands::respin(name, force, &cfg, &state_dir),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(e.exit_code());
    }
}

fn resolve_state_dir(flag: Option<&std::path::Path>) -> PathBuf {
    if let Some(dir) = flag {
        return dir.to_path_buf();
    }
    if let Ok(dir) = std::env::var("FATES_STATE_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from("/tmp/fates")
}
