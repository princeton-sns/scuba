use noise_kv::client::NoiseKVClient;
use noise_kv::data::NoiseData;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use std::sync::Arc;
use uuid::Uuid;

const LB_PREFIX: &str = "lightbulb";
const DEVICE_PREFIX: &str = "device";
const LB_DEVICE_VAL: &str = r#"{ "is_bulb": true }"#;
const LS_DEVICE_VAL: &str = r#"{ "is_bulb": false }"#;
const LB_OFF_VAL: &str = r#"{ "is_on": false }"#;
const LB_ON_VAL: &str = r#"{ "is_on": true }"#;

#[derive(Clone)]
struct LightswitchApp {
    client: NoiseKVClient,
}

impl LightswitchApp {
    pub async fn new() -> LightswitchApp {
        let client = NoiseKVClient::new(
            None, None,
            //Some("sns26.cs.princeton.edu"),
            //Some("8080"),
            // FIXME something isn't working anymore w the sns server
            // specifically
            false, None, None,
        )
        .await;
        Self { client }
    }

    // FIXME this should go into the noise-kv library and top-level functions
    // should return relevant Result
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
            Some(_) => {
                let device_guard = context.client.device.read();
                let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
                let val_opt = data_store_guard.get_data(&DEVICE_PREFIX.to_string());

                match val_opt {
                    Some(val) => {
                        if val.data_val() == LB_DEVICE_VAL {
                            return Ok(Some(String::from(
                                "Lightbulb device exists."
                            )));
                        } else if val.data_val() == LS_DEVICE_VAL {
                            return Ok(Some(String::from(
                                "Lightswitch device exists."
                            )));
                        } else {
                            panic!(
                                "A device exists that is neither lightbulb /
                                nor lightswitch."
                            );
                        }
                    },
                    None => panic!(
                        "Something went wrong with device initialization; /
                        this should never happen."
                    ),
                }
            },
            None => Ok(Some(String::from(
                "Device does not exist: please create either a lightbulb or a lightswitch to continue.",
            ))),
        }
    }

    pub async fn create_lightbulb_device(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();

        let id: String = DEVICE_PREFIX.to_owned();
        let json_val = LB_DEVICE_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), DEVICE_PREFIX.to_string(), json_val, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Lightbulb device created!"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create lightbulb device: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn create_lightswitch_device(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();

        let id: String = DEVICE_PREFIX.to_owned();
        let json_val = LS_DEVICE_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), DEVICE_PREFIX.to_string(), json_val, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Lightswitch device created!"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create lightswitch device: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn link_device(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        match context
            .client
            .create_linked_device(
                args.get_one::<String>("idkey").unwrap().to_string(),
            )
            .await
        {
            Ok(_) => Ok(Some(String::from("Linked device created!"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create linked device: {}",
                err.to_string()
            )))),
        }
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
        match context.client.add_contact(idkey.clone()).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Contact with idkey <{}> added",
                idkey
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not add contact: {}",
                err.to_string()
            )))),
        }
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

    pub fn get_groups(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let device_guard = context.client.device.read();
        let group_store_guard =
            device_guard.as_ref().unwrap().group_store.lock();
        let groups = group_store_guard.get_all_groups().values();

        Ok(Some(itertools::join(groups, "\n")))
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
            Some(val) => Ok(Some(String::from(format!("{}", val)))),
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

        let mut id: String = LB_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        let json_val = LB_OFF_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), LB_PREFIX.to_string(), json_val, None)
            .await
        {
            Ok(_) => Ok(Some(String::from(format!(
                "Created lightbulb with id {}",
                id
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not add lightbulb: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn share_lightbulb(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("lightbulb_id").unwrap().to_string();
        let names = args
            .get_many::<String>("names")
            .unwrap()
            .collect::<Vec<&String>>();
        match context.client.share_data(id.clone(), names.clone()).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Sharing lightbulb (id {}) with: \n{}",
                id,
                itertools::join(names, "\n")
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not share lightbulb: {}",
                err.to_string()
            )))),
        }
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
        let json_val = LB_ON_VAL.to_string();
        match context
            .client
            .set_data(id, LB_PREFIX.to_string(), json_val, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Turned light on"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not turn on lightbulb: {}",
                err.to_string()
            )))),
        }
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
        let json_val = LB_OFF_VAL.to_string();
        match context
            .client
            .set_data(id, LB_PREFIX.to_string(), json_val, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Turned light off"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not turn on lightbulb: {}",
                err.to_string()
            )))),
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
        .with_command_async(
            Command::new("create_lightbulb_device"),
            |_, context| {
                Box::pin(LightswitchApp::create_lightbulb_device(context))
            },
        )
        .with_command_async(
            Command::new("create_lightswitch_device"),
            |_, context| {
                Box::pin(LightswitchApp::create_lightswitch_device(context))
            },
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
        .with_command(Command::new("get_groups"), LightswitchApp::get_groups)
        .with_command(
            Command::new("get_lightbulb_state")
                .arg(Arg::new("lightbulb_id").required(true)),
            LightswitchApp::get_lightbulb_state,
        )
        .with_command_async(Command::new("add_lightbulb"), |_, context| {
            Box::pin(LightswitchApp::add_lightbulb(context))
        })
        .with_command_async(
            Command::new("share_lightbulb")
                .arg(
                    Arg::new("lightbulb_id")
                        .required(true)
                        .long("id")
                        .short('i'),
                )
                .arg(
                    Arg::new("names")
                        .required(true)
                        .long("name")
                        .short('n')
                        .action(ArgAction::Append),
                ),
            |args, context| {
                Box::pin(LightswitchApp::share_lightbulb(args, context))
            },
        )
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
