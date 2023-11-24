use actix_web::{App, HttpServer};
use clap::{Parser, Subcommand};

use noise_server_lib::sequencer;
use noise_server_lib::shard;

#[derive(Subcommand, Debug)]
enum Commands {
    Sequencer {
        #[arg(long)]
        port: u16,

        #[arg(long)]
        shard_count: u8,
    },

    Shard {
        #[arg(long)]
        port: u16,

        #[arg(long)]
        public_url: String,

        #[arg(long)]
        sequencer_url: String,

        #[arg(long)]
        inbox_count: u8,

        #[arg(long)]
        outbox_count: u8,
    },
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Sequencer { port, shard_count } => {
            let app_closure = sequencer::init(shard_count).await;

            HttpServer::new(move || App::new().configure(app_closure.clone()))
                .bind(("0.0.0.0", port))?
                .run()
                .await
        }

        Commands::Shard {
            port,
            public_url,
            sequencer_url,
            inbox_count,
            outbox_count,
        } => {
            let app_closure =
                shard::init(public_url, sequencer_url, inbox_count, outbox_count).await;

            HttpServer::new(move || App::new().configure(app_closure.clone()))
                .bind(("0.0.0.0", port))?
                .run()
                .await
        }
    }
}
