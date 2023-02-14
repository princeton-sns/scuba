#![feature(async_closure)]

use std::collections::HashMap;
use noise_core::core::Core;
use futures::channel::mpsc;
use repl_rs::{Value, Repl, Command, Parameter};
use repl_rs::Result as ReplResult;
use std::sync::Arc;
use tokio::task;

const BUFFER_SIZE: usize = 10;

struct LightswitchApp {
  receiver: mpsc::Receiver<(String, String)>,
  core: Core,
}

impl LightswitchApp {
  pub fn new() -> LightswitchApp {
    let (sender, receiver) = mpsc::channel::<(String, String)>(BUFFER_SIZE);
    Self {
      receiver,
      core: Core::new(None, None, false, sender),
    }
  }

  // TODO Arc makes Self immutable, so need to use interior mutability
  pub async fn receive_messages(&self) {
    //self.core.receive_message().await;
    println!("Receiving!");
  }

  pub fn hello(
      args: HashMap<String, Value>,
      _context: &mut Arc<Self>
  ) -> ReplResult<Option<String>> {
    Ok(Some(format!("Hello, {}", args["who"])))
  }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(LightswitchApp::new());

    let app_listener = app.clone();
    task::spawn_blocking(async move || {
      //app_listener.receive_messages().await;
      println!("Blocking");
    }).await;
    
    let mut repl = Repl::new(app)
        .with_name("Lightswitch App")
        .with_version("v0.1.0")
        .with_description("Noise lightswitch app")
        .add_command(
            Command::new("hello", LightswitchApp::hello)
                .with_parameter(Parameter::new("who").set_required(true)?)?
                .with_help("Greetings!"),
   );

   task::spawn(async {
     repl.run()
   }).await.unwrap()
}

