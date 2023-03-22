use noise_kv::client::NoiseKVClient;
use noise_kv::data::BasicData;
use reedline_repl_rs::clap::{Arg, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
struct LightswitchApp {
    client: NoiseKVClient,
}

//#[derive(Serialize, Deserialize)]
//enum Operation {
//    On,
//    Off,
//}
//
//impl Operation {
//    fn to_string(op: Operation) -> String {
//        serde_json::to_string(&op).unwrap()
//    }
//
//    fn from_string(string: String) -> Operation {
//        serde_json::from_str(string.as_str()).unwrap()
//    }
//}

impl LightswitchApp {
    pub async fn new() -> LightswitchApp {
        let client = NoiseKVClient::new(None, None, false, None).await;
        client.create_standalone_device();
        Self { client }
    }

    pub fn check_device(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        match context.client.device.read().as_ref() {
            Some(_) => Ok(Some(String::from("Device exists."))),
            None => Ok(Some(String::from(
                "Device does NOT exist: please create one.",
            ))),
        }
    }

    pub fn create_device(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();
        Ok(Some(String::from("Device created!")))
    }

    pub fn get_name(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from(format!(
            "Name: {}",
            context.client.linked_name()
        ))))
    }

    pub fn get_idkey(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from(format!(
            "Idkey: {}",
            context.client.idkey()
        ))))
    }

    pub fn get_contacts(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(itertools::join(&context.client.get_contacts(), "\n")))
    }

    pub async fn add_contact(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
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
        let data = context
            .client
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_all_data();
        // TODO iterate + print
        Ok(Some(String::from("Not yet implemented")))
    }

    pub async fn add_lightbulb(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let id = Uuid::new_v4().to_string();
        let json_val = r#"{ "is_on": false }"#;
        let lightbulb = BasicData::new(id, json_val.to_string());
        // TODO set for all linked devices

        Ok(Some(String::from("Not yet implemented")))
    }

    pub async fn turn_on(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from("Not yet implemented")))
    }

    pub async fn turn_off(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from("Not yet implemented")))
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
        .with_command(Command::new("get_data"), LightswitchApp::get_data)
        .with_command_async(Command::new("add_lightbulb"), |_, context| {
            Box::pin(LightswitchApp::add_lightbulb(context))
        })
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(LightswitchApp::add_contact(args, context))
            },
        )
        .with_command_async(Command::new("turn_on"), |_, context| {
            Box::pin(LightswitchApp::turn_on(context))
        })
        .with_command_async(Command::new("turn_off"), |_, context| {
            Box::pin(LightswitchApp::turn_off(context))
        });

    repl.run_async().await
}
