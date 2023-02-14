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
    loop {
      self.core.receive_message().await;
    }
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

    let app_future = app_listener.receive_messages();

    let (repl_res, app_res) = futures::join!(repl_future, app_future);
    // FIXME check results
    Ok(())
}

