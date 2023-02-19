use async_trait::async_trait;
use futures::channel::mpsc;
use noise_core::core::{Core, CoreClient};
use reedline_repl_rs::clap::Command;
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task;

const BUFFER_SIZE: usize = 10;

struct LightswitchClient {}

#[async_trait]
impl CoreClient for LightswitchClient {
    async fn client_callback(&self, sender: String, message: String) {
        println!("in LIGHTSWITCH client_callback");
        println!("sender: {:?}", sender);
        println!("message: {:?}", message);
    }
}

struct LightswitchApp {
    core: Arc<Core<LightswitchClient>>,
    light: bool,
}

impl LightswitchApp {
    pub async fn new() -> LightswitchApp {
        let lightswitch_client = Arc::new(LightswitchClient {});
        Self {
            core: Core::new(None, None, false, Some(lightswitch_client)).await,
            light: false,
        }
    }

    async fn send_message(
        &self,
        message: &String,
    ) -> reqwest::Result<reqwest::Response> {
        self.core
            .send_message(vec![self.core.idkey()], message)
            .await
    }

    pub async fn on(context: &mut Arc<Self>) -> ReplResult<Option<String>> {
        //if context.light {
        //    return Ok(Some(format!("Light is already on")));
        //}
        //context.light = true;

        let message = String::from("on");
        context.send_message(&message).await;

        Ok(Some(format!("Turning light on")))
    }

    pub async fn off(context: &mut Arc<Self>) -> ReplResult<Option<String>> {
        //if !context.light {
        //    return Ok(Some(format!("Light is already off")));
        //}
        //context.light = false;

        let message = String::from("off");
        context.send_message(&message).await;

        Ok(Some(format!("Turning light off")))
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(LightswitchApp::new().await);

    let app_listener = app.clone();

    let mut repl = Repl::new(app)
        .with_name("Lightswitch App")
        .with_version("v0.1.0")
        .with_description("Noise lightswitch app")
        .with_command_async(Command::new("on"), |_, context| {
            Box::pin(LightswitchApp::on(context))
        })
        .with_command_async(Command::new("off"), |_, context| {
            Box::pin(LightswitchApp::off(context))
        });

    repl.run_async().await
}
