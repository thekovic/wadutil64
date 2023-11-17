use wadutil64::*;
use std::process::ExitCode;

#[derive(clap::Parser)]
struct Args {
    /// Show extra debugging info
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Extracts IWAD data from ROM
    Extract(extract::Args),
    /// Builds IWAD from local data
    Build(build::Args),
    /// Inspects ROM or IWAD
    Inspect(inspect::Args),
}

fn main() -> ExitCode {
    let args: Args = clap::Parser::parse();

    let level = match args.verbose {
        true => log::LevelFilter::Debug,
        false => log::LevelFilter::Info,
    };
    pretty_env_logger::formatted_builder()
        .filter_level(level)
        .filter_module("ghakuf", log::LevelFilter::Off)
        .parse_env("RUST_LOG")
        .target(pretty_env_logger::env_logger::Target::Stdout)
        .init();

    let res = match args.command {
        Commands::Extract(args) => extract::extract(args),
        Commands::Build(args) => build::build(args),
        Commands::Inspect(args) => inspect::inspect(args),
    };
    match res {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}
