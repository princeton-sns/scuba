use noise_kv::client::NoiseKVClient;
use reedline_repl_rs::clap::{Arg, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
//use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

//#[derive(Debug)]
//enum LightswitchError {
//    ReplError(reedline_repl_rs::Error),
//    DeviceDoesNotExist,
//}
//
//impl From<reedline_repl_rs::Error> for LightswitchError {
//    fn from(e: reedline_repl_rs::Error) -> Self {
//        LightswitchError::ReplError(e)
//    }
//}
//
//impl fmt::Display for LightswitchError {
//    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//        match self {
//            LightswitchError::ReplError(e) => write!(f, "REPL Error: {}",
// e),            LightswitchError::DeviceDoesNotExist => {
//                write!(f, "Device does not exist, cannot run command.")
//            }
//        }
//    }
//}
//
//impl std::error::Error for LightswitchError {}

#[derive(Clone)]
struct LightswitchApp {
    client: NoiseKVClient,
    // TODO is_lightbulb flag for differentiating between a lightswitch
    // and a lightbulb (where both need to know the state of the lightbulb)
}

impl LightswitchApp {
    pub async fn new() -> LightswitchApp {
        let client = NoiseKVClient::new(None, None, false, None).await;
        Self { client }
    }

    fn exists_device(&self) -> bool {
        match self.client.device.read().as_ref() {
            Some(_) => true,
            None => false,
        }
    }

    pub fn check_device(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        match context.client.device.read().as_ref() {
            Some(_) => Ok(Some(String::from("Device exists."))),
            None => Ok(Some(String::from(
                "Device does not exist: please create one.",
            ))),
        }
    }

    pub fn create_device(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();
        Ok(Some(String::from("Standalone device created!")))
    }

    pub async fn link_device(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context
            .client
            .create_linked_device(
                args.get_one::<String>("idkey").unwrap().to_string(),
            )
            .await;
        Ok(Some(String::from("Linked device created!")))
    }

    pub fn get_name(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        Ok(Some(String::from(format!(
            "Name: {}",
            context.client.linked_name()
        ))))
    }

    pub fn get_idkey(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        Ok(Some(String::from(format!(
            "Idkey: {}",
            context.client.idkey()
        ))))
    }

    pub fn get_linked_devices(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        Ok(Some(itertools::join(
            &context
                .client
                .device
                .read()
                .as_ref()
                .unwrap()
                .linked_devices(),
            "\n",
        )))
    }

    pub fn get_contacts(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        Ok(Some(itertools::join(&context.client.get_contacts(), "\n")))
    }

    pub async fn add_contact(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let idkey = args.get_one::<String>("idkey").unwrap().to_string();
        context.client.add_contact(idkey.clone()).await;
        Ok(Some(String::from(format!(
            "Contact with idkey <{}> added",
            idkey
        ))))
    }

    pub fn get_data(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let data = data_store_guard.get_all_data().values();

        Ok(Some(itertools::join(data, "\n")))
    }

    pub fn get_lightbulb_state(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("lightbulb_id").unwrap().to_string();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let val_opt = data_store_guard.get_data(&id);

        match val_opt {
            Some(val) => Ok(Some(String::from(format!("State: \n{}", val)))),
            None => Ok(Some(String::from(format!(
                "Lightbulb with id {} does not exist.",
                id,
            )))),
        }
    }

    pub async fn add_lightbulb(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let mut id: String = "lightbulb".to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        let json_val = r#"{ "is_on": false }"#.to_string();
        context.client.set_data(id.clone(), json_val, None).await;

        Ok(Some(String::from(format!(
            "Created lightbulb with id {}",
            id
        ))))
    }

    pub async fn turn_on(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("lightbulb_id").unwrap().to_string();
        let json_val = r#"{ "is_on": true }"#.to_string();
        context.client.set_data(id, json_val, None).await;

        Ok(Some(String::from("Turned light on")))
    }

    pub async fn turn_off(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("lightbulb_id").unwrap().to_string();
        let json_val = r#"{ "is_on": false }"#.to_string();
        context.client.set_data(id, json_val, None).await;

        Ok(Some(String::from("Turned light off")))
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(LightswitchApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Lightswitch App")
        .with_version("v0.1.0")
        .with_description("Noise lightswitch app")
        .with_command(
            Command::new("create_device"),
            LightswitchApp::create_device,
        )
        .with_command_async(
            Command::new("link_device").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(LightswitchApp::link_device(args, context))
            },
        )
        .with_command(
            Command::new("check_device"),
            LightswitchApp::check_device,
        )
        .with_command(Command::new("get_name"), LightswitchApp::get_name)
        .with_command(Command::new("get_idkey"), LightswitchApp::get_idkey)
        .with_command(
            Command::new("get_contacts"),
            LightswitchApp::get_contacts,
        )
        .with_command(
            Command::new("get_linked_devices"),
            LightswitchApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), LightswitchApp::get_data)
        .with_command(
            Command::new("get_lightbulb_state")
                .arg(Arg::new("lightbulb_id").required(true)),
            LightswitchApp::get_lightbulb_state,
        )
        .with_command_async(Command::new("add_lightbulb"), |_, context| {
            Box::pin(LightswitchApp::add_lightbulb(context))
        })
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(LightswitchApp::add_contact(args, context))
            },
        )
        .with_command_async(
            Command::new("turn_on")
                .arg(Arg::new("lightbulb_id").required(true)),
            |args, context| Box::pin(LightswitchApp::turn_on(args, context)),
        )
        .with_command_async(
            Command::new("turn_off")
                .arg(Arg::new("lightbulb_id").required(true)),
            |args, context| Box::pin(LightswitchApp::turn_off(args, context)),
        );

    repl.run_async().await
}
