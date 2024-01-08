use actix_web::{App, HttpServer};
use clap::{Parser, Subcommand};

use scuba_server_lib::attestation_proxy;
use scuba_server_lib::sequencer;
use scuba_server_lib::shard;

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

        #[arg(long)]
        isb_socket_port: Option<u16>,

        #[arg(long)]
        isb_socket_addr: Option<String>,

        #[arg(long, action)]
        inbox_drop_messages: bool,

        #[arg(long)]
        isb_chunk_size: Option<usize>,
    },

    AttestationProxy {
        #[arg(long)]
        port: u16,

        #[arg(long)]
        sequencer_url: String,
    },

    SingleShard {
        #[arg(long)]
        sequencer_port: u16,

        #[arg(long)]
        shard_port: u16,

        #[arg(long)]
        public_shard_url: String,

        #[arg(long)]
        attestation_proxy_port: Option<u16>,
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
            isb_socket_addr,
            isb_socket_port,
            inbox_drop_messages,
            isb_chunk_size,
        } => {
            if (isb_socket_addr.is_some() as u8)
                ^ (isb_socket_port.is_some() as u8)
                != 0
            {
                panic!("Must set either none or both of --isb-socket-addr and --isb-socket-port!");
            }

            let isb_socket_spec =
                isb_socket_port.map(|port| (port, isb_socket_addr.unwrap()));
            let app_closure = shard::init(
                public_url,
                sequencer_url,
                inbox_count,
                outbox_count,
                isb_socket_spec,
                false,
                inbox_drop_messages,
                isb_chunk_size,
            )
            .await;

            HttpServer::new(move || App::new().configure(app_closure.clone()))
                .bind(("0.0.0.0", port))?
                .run()
                .await
        }

        Commands::AttestationProxy {
            port,
            sequencer_url,
        } => {
            let att_closure = attestation_proxy::init(&sequencer_url).await;

            HttpServer::new(move || App::new().configure(att_closure.clone()))
                .bind(("0.0.0.0", port))?
                .run()
                .await
        }

        Commands::SingleShard {
            sequencer_port,
            shard_port,
            public_shard_url,
            attestation_proxy_port,
        } => tokio::try_join!(
            async move {
                let sequencer_closure = sequencer::init(1).await;

                HttpServer::new(move || {
                    App::new().configure(sequencer_closure.clone())
                })
                .bind(("127.0.0.1", sequencer_port))?
                .run()
                .await
            },
            async move {
                let shard_closure = shard::init(
                    public_shard_url,
                    format!("http://127.0.0.1:{}", sequencer_port),
                    1,
                    1,
                    None,
                    true,
                    false,
                    None,
                )
                .await;

                HttpServer::new(move || {
                    App::new().configure(shard_closure.clone())
                })
                .bind(("0.0.0.0", shard_port))?
                .run()
                .await
            },
            async move {
                if let Some(att_port) = attestation_proxy_port {
                    let att_closure = attestation_proxy::init(&format!(
                        "http://127.0.0.1:{}",
                        sequencer_port
                    ))
                    .await;

                    HttpServer::new(move || {
                        App::new().configure(att_closure.clone())
                    })
                    .bind(("0.0.0.0", att_port))?
                    .run()
                    .await
                } else {
                    Ok(())
                }
            },
        )
        .map(|_| ()),
    }
}
