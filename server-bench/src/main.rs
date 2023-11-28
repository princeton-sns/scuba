use clap::{Parser, Subcommand};

mod delete_attest_messages_bench;

#[derive(Subcommand, Debug)]
enum Commands {
    DeleteAttestMessagesBench(delete_attest_messages_bench::Opts),
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    match args.command {
        Commands::DeleteAttestMessagesBench(opts) => {
            delete_attest_messages_bench::run(opts).await
        }
    }
}
