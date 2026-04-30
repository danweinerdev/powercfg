// Temporary allows: the new modules are not yet referenced from main(); task
// 1.3 wires the clap dispatcher and removes both attributes.
#![allow(dead_code)]
#![allow(unused_imports)]

mod format;
mod paths;
mod source;
mod time;

fn main() -> std::process::ExitCode {
    eprintln!("powercfg: CLI not yet wired up (task 1.3 lands the clap surface)");
    std::process::ExitCode::from(1)
}
