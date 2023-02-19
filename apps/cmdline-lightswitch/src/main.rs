use async_trait::async_trait;
use noise_core::core::{Core, CoreClient};
use parking_lot::RwLock;
use reedline_repl_rs::clap::Command;
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use yansi::Paint;

fn paint_red_bold(input: &str) -> String {
    Box::new(Paint::red(input).bold()).to_string()
}

fn paint_cyan_bold(input: &str) -> String {
    Box::new(Paint::cyan(input).bold()).to_string()
}

fn paint_white_bold(input: &str) -> String {
    Box::new(Paint::white(input).bold()).to_string()
}

#[derive(Clone)]
struct LightswitchApp {
    core: Option<Arc<Core<LightswitchApp>>>,
    light: Arc<RwLock<bool>>,
}

#[derive(Serialize, Deserialize)]
enum Operation {
    On,
    Off,
}

impl Operation {
    fn to_string(op: Operation) -> String {
        serde_json::to_string(&op).unwrap()
    }

    fn from_string(string: String) -> Operation {
        serde_json::from_str(string.as_str()).unwrap()
    }
}

#[async_trait]
impl CoreClient for LightswitchApp {
    async fn client_callback(&self, sender: String, message: String) {
        println!("{}", paint_white_bold(format!("FROM: {}", sender).as_str()));
        match Operation::from_string(message) {
            Operation::On => {
                if *self.light.read() {
                    println!("{}", paint_red_bold("Light is already on"));
                } else {
                    println!("{}", paint_cyan_bold("Turning light on"));
                    *self.light.write() = true;
                }
            }
            Operation::Off => {
                if !*self.light.read() {
                    println!("{}", paint_red_bold("Light is already off"));
                } else {
                    println!("{}", paint_cyan_bold("Turning light off"));
                    *self.light.write() = false;
                }
            }
        }
    }
}

impl LightswitchApp {
    pub async fn new() -> LightswitchApp {
        let mut lightswitch_app = LightswitchApp {
            core: None,
            light: Arc::new(RwLock::new(false)),
        };
        let core = Core::new(
            None,
            None,
            false,
            Some(Arc::new(lightswitch_app.clone())),
        )
        .await;
        lightswitch_app.core = Some(core);
        lightswitch_app
    }

    async fn send_message(
        &self,
        message: &String,
    ) -> reqwest::Result<reqwest::Response> {
        let idkey = self.core.as_ref().unwrap().idkey().to_string();
        self.core
            .as_ref()
            .unwrap()
            .send_message(vec![idkey], message)
            .await
    }

    pub async fn on(context: &mut Arc<Self>) -> ReplResult<Option<String>> {
        match context
            .send_message(&Operation::to_string(Operation::On))
            .await
        {
            Ok(_) => Ok(None),
            Err(err) => panic!("Error sending message to server: {:?}", err),
        }
    }

    pub async fn off(context: &mut Arc<Self>) -> ReplResult<Option<String>> {
        match context
            .send_message(&Operation::to_string(Operation::Off))
            .await
        {
            Ok(_) => Ok(None),
            Err(err) => panic!("Error sending message to server: {:?}", err),
        }
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(LightswitchApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Lightswitch App")
        .with_version("v0.1.0")
        .with_description("Noise lightswitch app")
        .with_banner(
            paint_white_bold(
                format!("IDKEY: {}", app.core.as_ref().unwrap().idkey())
                    .as_str(),
            )
            .as_str(),
        )
        .with_command_async(Command::new("on"), |_, context| {
            Box::pin(LightswitchApp::on(context))
        })
        .with_command_async(Command::new("off"), |_, context| {
            Box::pin(LightswitchApp::off(context))
        });

    repl.run_async().await
}
