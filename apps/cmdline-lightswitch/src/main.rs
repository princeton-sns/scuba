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
            false, None, None, //Some(1),
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

    pub async fn create_lightbulb(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();

        let id: String = DEVICE_PREFIX.to_owned();
        let json_string = LB_DEVICE_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), DEVICE_PREFIX.to_string(), json_string, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Lightbulb device created!"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create lightbulb device: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn create_lightswitch(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();

        let id: String = DEVICE_PREFIX.to_owned();
        let json_string = LS_DEVICE_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), DEVICE_PREFIX.to_string(), json_string, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Lightswitch device created!"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create lightswitch device: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn create_linked_lightswitch(
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

    pub fn get_perms(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let device_guard = context.client.device.read();
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let perms = meta_store_guard.get_all_perms().values();

        Ok(Some(itertools::join(perms, "\n")))
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
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let groups = meta_store_guard.get_all_groups().values();

        Ok(Some(itertools::join(groups, "\n")))
    }

    pub fn get_state(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("id").unwrap().to_string();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let val_opt = data_store_guard.get_data(&id);

        match val_opt {
            Some(val) => Ok(Some(String::from(format!("{}", val)))),
            None => Ok(Some(String::from(format!(
                "Datum with id {} does not exist.",
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
        let json_string = LB_OFF_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), LB_PREFIX.to_string(), json_string, None)
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

    pub async fn set_state(
        args: ArgMatches,
        bulb_state_str: &str,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("id").unwrap().to_string();
        let json_string = bulb_state_str.to_string();
        match context
            .client
            .set_data(id.clone(), LB_PREFIX.to_string(), json_string, None)
            .await
        {
            Ok(_) => Ok(Some(String::from(format!(
                "Set state for bulb with id {}",
                id
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not set bulb state: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn share(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("id").unwrap().to_string();

        if let Some(arg_readers) = args.get_many::<String>("readers") {
            let readers = arg_readers.collect::<Vec<&String>>();
            let res = context
                .client
                .add_readers(id.clone(), readers.clone())
                .await;
            if res.is_err() {
                return Ok(Some(String::from(format!(
                    "Error adding readers to datum: {}",
                    res.err().unwrap().to_string()
                ))));
            }
        }

        if let Some(arg_writers) = args.get_many::<String>("writers") {
            let writers = arg_writers.collect::<Vec<&String>>();
            let res = context
                .client
                .add_writers(id.clone(), writers.clone())
                .await;
            if res.is_err() {
                return Ok(Some(String::from(format!(
                    "Error adding writers to datum: {}",
                    res.err().unwrap().to_string()
                ))));
            }
        }

        Ok(Some(String::from(format!(
            "Successfully shared datum {}",
            id
        ))))
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(LightswitchApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Lightswitch App")
        .with_version("v0.1.0")
        .with_description("Noise lightswitch app")
        .with_command_async(Command::new("create_lightbulb"), |_, context| {
            Box::pin(LightswitchApp::create_lightbulb(context))
        })
        .with_command_async(Command::new("create_lightswitch"), |_, context| {
            Box::pin(LightswitchApp::create_lightswitch(context))
        })
        .with_command_async(
            Command::new("create_linked_lightswitch")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(LightswitchApp::create_linked_lightswitch(
                    args, context,
                ))
            },
        )
        .with_command(
            Command::new("check_device"),
            LightswitchApp::check_device,
        )
        .with_command(Command::new("get_name"), LightswitchApp::get_name)
        .with_command(Command::new("get_idkey"), LightswitchApp::get_idkey)
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(LightswitchApp::add_contact(args, context))
            },
        )
        .with_command(
            Command::new("get_contacts"),
            LightswitchApp::get_contacts,
        )
        .with_command(
            Command::new("get_linked_devices"),
            LightswitchApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), LightswitchApp::get_data)
        .with_command(Command::new("get_perms"), LightswitchApp::get_perms)
        .with_command(Command::new("get_groups"), LightswitchApp::get_groups)
        .with_command(
            Command::new("get_state").arg(Arg::new("id").required(true)),
            LightswitchApp::get_state,
        )
        .with_command_async(Command::new("add_lightbulb"), |_, context| {
            Box::pin(LightswitchApp::add_lightbulb(context))
        })
        .with_command_async(
            Command::new("share")
                .arg(Arg::new("id").required(true).long("id").short('i'))
                .arg(
                    Arg::new("readers")
                        .required(true)
                        .long("readers")
                        .short('r')
                        .action(ArgAction::Append),
                )
                .arg(
                    Arg::new("writers")
                        .required(true)
                        .long("writers")
                        .short('w')
                        .action(ArgAction::Append),
                ),
            |args, context| Box::pin(LightswitchApp::share(args, context)),
        )
        .with_command_async(
            Command::new("turn_on").arg(Arg::new("id").required(true)),
            |args, context| {
                Box::pin(LightswitchApp::set_state(args, LB_ON_VAL, context))
            },
        )
        .with_command_async(
            Command::new("turn_off").arg(Arg::new("id").required(true)),
            |args, context| {
                Box::pin(LightswitchApp::set_state(args, LB_OFF_VAL, context))
            },
        );

    repl.run_async().await
}
