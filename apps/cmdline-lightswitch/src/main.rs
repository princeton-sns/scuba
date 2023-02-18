#![feature(async_closure)]

use async_trait::async_trait;
use futures::channel::mpsc;
use noise_core::core::{Core, CoreClient};
use repl_rs::Result as ReplResult;
use repl_rs::{Command, Parameter, Repl, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task;

const BUFFER_SIZE: usize = 10;

struct LightswitchClient {}

#[async_trait]
impl CoreClient for LightswitchClient {
    async fn client_callback(&self, sender: String, message: String) {
        println!("in LIGHTSWITCH client_callback");
    }
}

struct LightswitchApp {
    core: Arc<Core<LightswitchClient>>,
}

impl LightswitchApp {
    pub async fn new() -> LightswitchApp {
        let lightswitch_client = Arc::new(LightswitchClient {});
        Self {
            core: Core::new(None, None, false, Some(lightswitch_client)).await,
        }
    }

    pub fn hello(
        args: HashMap<String, Value>,
        _context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(format!("Hello, {}", args["who"])))
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(LightswitchApp::new().await);

    let app_listener = app.clone();

    let repl_future = task::spawn_blocking(move || {
        let mut repl = Repl::new(app)
            .with_name("Lightswitch App")
            .with_version("v0.1.0")
            .with_description("Noise lightswitch app")
            .add_command(
                Command::new("hello", LightswitchApp::hello)
                    .with_parameter(Parameter::new("who").set_required(true)?)?
                    .with_help("Greetings!"),
            );

        repl.run()
    });

    if let (Err(err),) = futures::join!(repl_future) {
        panic!("Error joining threads: {:?}", err);
    }

    Ok(())
}
