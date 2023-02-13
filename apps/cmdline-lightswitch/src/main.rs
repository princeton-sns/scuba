use std::collections::HashMap;
use noise_core::core::Core;
use futures::channel::mpsc;
use repl_rs::{Value, Repl, Command, Parameter};
use repl_rs::Result as ReplResult;
use std::sync::Arc;

struct LightswitchApp {
  //receiver: mpsc::Receiver<(String, String)>,
  core: Core,
}

impl LightswitchApp {
  pub fn new(sender: mpsc::Sender<(String, String)>) -> LightswitchApp {
    LightswitchApp {
      core: Core::new(None, None, false, sender),
    }
  }

  pub fn hello(args: HashMap<String, Value>, _context: &mut Arc<Self>) -> ReplResult<Option<String>> {
    Ok(Some(format!("Hello, {}", args["who"])))
  }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    println!("Hello, world!");
    let (sender, receiver) = mpsc::channel::<(String, String)>(10);
    let app = Arc::new(LightswitchApp::new(sender));
    //let app_listener = app.clone();
    //task::spawn_blocking(
    //  app_listener.receive_messages();
    //).await;
    
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
}
