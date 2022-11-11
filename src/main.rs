use std::path::PathBuf;
use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    /// Stdio socket file
    #[arg(long, env = "DOLOROUS_SOCKET", value_name = "FILE")]
    socket: Option<PathBuf>,
    /// Configuration file
    #[arg(short, long, env = "DOLOROUS_CONFIG", value_name = "FILE", default_value = "/etc/dolorous/config.toml")]
    config: PathBuf,
}

fn main() {
    let args = Args::parse();
}
