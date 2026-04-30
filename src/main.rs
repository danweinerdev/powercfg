// The 1.2 utility modules (`paths`, `time`, `format::*`, `source::error`) are
// declared and exercised by their own unit tests but not yet referenced from
// any command handler — `sleepstates` is a no-op stub at 1.3 and the other
// five `unimplemented!()`. Task 1.4 wires the first real call sites and the
// item-level dead code disappears; until then this lid keeps `cargo build`
// warning-free without forcing every utility item to carry its own allow.
#![allow(dead_code)]

mod cli;
mod cmd;
mod format;
mod paths;
mod source;
mod time;

use clap::Parser;

fn main() -> std::process::ExitCode {
    // Initialize tracing-subscriber early so source-layer debug! calls
    // are observable when RUST_LOG is set.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = cli::Cli::parse();

    // Each variant destructures into the matching `cmd::*::Args` struct.
    // Future args (Format, SysRoot, etc.) extend the Args struct rather than
    // threading new positional params through six dispatch arms.
    let result = match cli.command {
        cli::Command::Requests { verbose } => cmd::requests::run(cmd::requests::Args { verbose }),
        cli::Command::Lastwake { verbose, history } => {
            cmd::lastwake::run(cmd::lastwake::Args { verbose, history })
        }
        cli::Command::Devicequery {
            verbose,
            enabled_only,
        } => cmd::devicequery::run(cmd::devicequery::Args {
            verbose,
            enabled_only,
        }),
        cli::Command::Sleepstates { verbose } => {
            cmd::sleepstates::run(cmd::sleepstates::Args { verbose })
        }
        cli::Command::Waketimers { verbose } => {
            cmd::waketimers::run(cmd::waketimers::Args { verbose })
        }
        cli::Command::Energy { verbose } => cmd::energy::run(cmd::energy::Args { verbose }),
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::ExitCode::from(1)
        }
    }
}
