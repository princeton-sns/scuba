use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use single_key_tank::client::TankClient;
use single_key_tank::data::ScubaData;
use std::sync::Arc;
use uuid::Uuid;

const FLOW_PREFIX: &str = "flow";
const LIGHT_FLOW: &str = r#"{ "flow": "light" }"#;
const MOD_FLOW: &str = r#"{ "flow": "moderate" }"#;
const HEAVY_FLOW: &str = r#"{ "flow": "heavy" }"#;

const SYMPTOMS_PREFIX: &str = "symptoms";
const EMPTY_SYMPTOMS_VAL: &str = r#"{ "symptoms": [] }"#;

#[derive(Clone)]
struct PeriodTrackingApp {
    client: TankClient,
}

impl PeriodTrackingApp {
    pub async fn new() -> PeriodTrackingApp {
        let client = TankClient::new(
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
            Some(_) => Ok(Some(String::from("Device exists"))),
            None => Ok(Some(String::from(
                "Device does not exist: please create one to continue.",
            ))),
        }
    }

    pub fn create_new_device(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();
        Ok(Some(String::from("Standalone device created.")))
    }

    pub async fn create_linked_device(
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

    pub async fn add_flow(
        args: ArgMatches,
        flow_str: &str,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let mut id: String;
        if let Some(arg_id) = args.get_one::<String>("flow_id") {
            id = arg_id.to_string();
        } else {
            id = FLOW_PREFIX.to_owned();
            id.push_str("/");
            id.push_str(&Uuid::new_v4().to_string());
        }
        let json_string = flow_str.to_string();

        match context
            .client
            .set_data(id.clone(), FLOW_PREFIX.to_string(), json_string, None)
            .await
        {
            Ok(_) => {
                Ok(Some(String::from(format!("Added flow with id {}", id))))
            }
            Err(err) => Ok(Some(String::from(format!(
                "Error adding flow: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn add_symptoms(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let mut id: String;
        let json_string: String;
        // modify existing datum
        if let Some(arg_id) = args.get_one::<String>("symptoms_id") {
            id = arg_id.to_string();

            // get existing data, if it exists
            let device_guard = context.client.device.read();
            let data_store_guard =
                device_guard.as_ref().unwrap().data_store.read();
            let val_opt = data_store_guard.get_data(&id);

            if val_opt.is_none() {
                return Ok(Some(String::from(format!(
                    "Datum with id {} does not exist.",
                    id,
                ))));
            }

            let existing_val = val_opt.unwrap();

            if let Some(arg_symptoms) = args.get_many::<String>("symptoms_list")
            {
                // append new symptoms to list of old symptoms
                let mut new_symptoms_obj =
                    serde_json::json!(arg_symptoms.collect::<Vec<&String>>());
                let mut new_symptoms = new_symptoms_obj.as_array_mut().unwrap();
                let mut existing_symptoms_obj: serde_json::Value =
                    serde_json::from_str(existing_val.data_val()).unwrap();
                let existing_symptoms = &mut existing_symptoms_obj["symptoms"]
                    .as_array_mut()
                    .unwrap();
                existing_symptoms.append(&mut new_symptoms);

                let json_val = serde_json::json!({
                    "symptoms": existing_symptoms,
                });
                json_string = serde_json::to_string(&json_val).unwrap();
            } else {
                // keep existing data so it is not overwritten
                json_string = existing_val.data_val().to_string();
            }
        // create new datum
        } else {
            id = SYMPTOMS_PREFIX.to_owned();
            id.push_str("/");
            id.push_str(&Uuid::new_v4().to_string());

            if let Some(arg_symptoms) = args.get_many::<String>("symptoms_list")
            {
                let symptoms = arg_symptoms.collect::<Vec<&String>>();
                let json_val = serde_json::json!({
                    "symptoms": symptoms,
                });
                json_string = serde_json::to_string(&json_val).unwrap();
            } else {
                json_string = EMPTY_SYMPTOMS_VAL.to_string();
            }
        }

        match context
            .client
            .set_data(
                id.clone(),
                SYMPTOMS_PREFIX.to_string(),
                json_string,
                None,
            )
            .await
        {
            Ok(_) => {
                Ok(Some(String::from(format!("Added symptoms with id {}", id))))
            }
            Err(err) => Ok(Some(String::from(format!(
                "Error adding symptoms: {}",
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
    let app = Arc::new(PeriodTrackingApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("PeriodTracking App")
        .with_version("v0.1.0")
        .with_description("Scuba period tracking app")
        .with_command(
            Command::new("create_new_device"),
            PeriodTrackingApp::create_new_device,
        )
        .with_command_async(
            Command::new("create_linked_device")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(PeriodTrackingApp::create_linked_device(args, context))
            },
        )
        .with_command(
            Command::new("check_device"),
            PeriodTrackingApp::check_device,
        )
        .with_command(Command::new("get_name"), PeriodTrackingApp::get_name)
        .with_command(Command::new("get_idkey"), PeriodTrackingApp::get_idkey)
        .with_command(
            Command::new("get_contacts"),
            PeriodTrackingApp::get_contacts,
        )
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(PeriodTrackingApp::add_contact(args, context))
            },
        )
        .with_command(
            Command::new("get_linked_devices"),
            PeriodTrackingApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), PeriodTrackingApp::get_data)
        .with_command(Command::new("get_perms"), PeriodTrackingApp::get_perms)
        .with_command(Command::new("get_groups"), PeriodTrackingApp::get_groups)
        .with_command(
            Command::new("get_state")
                .arg(Arg::new("id").required(true)),
            PeriodTrackingApp::get_state,
        )
        .with_command_async(
            Command::new("add_light_flow")
                .about("Creates new datum if 'id' is omitted, else modifies existing datum")
                .arg(Arg::new("flow_id").long("id").short('i').required(false)), 
            |args, context| {
                Box::pin(PeriodTrackingApp::add_flow(args, LIGHT_FLOW, context))
            }
        )
        .with_command_async(
            Command::new("add_mod_flow")
                .about("Creates new datum if 'id' is omitted, else modifies existing datum")
                .arg(Arg::new("flow_id").long("id").short('i').required(false)), 
            |args, context| {
                Box::pin(PeriodTrackingApp::add_flow(args, MOD_FLOW, context))
            }
        )
        .with_command_async(
            Command::new("add_heavy_flow")
                .about("Creates new datum if 'id' is omitted, else modifies existing datum")
                .arg(Arg::new("flow_id").long("id").short('i').required(false)), 
            |args, context| {
                Box::pin(PeriodTrackingApp::add_flow(args, HEAVY_FLOW, context))
            }
        )
        .with_command_async(
            Command::new("add_symptoms")
                .about("Creates new datum if 'id' is omitted, else modifies existing datum")
                .arg(Arg::new("symptoms_id").long("id").short('i').required(false))
                .arg(Arg::new("symptoms_list").long("symptoms").short('s').required(false).action(ArgAction::Append)),
            |args, context| {
                Box::pin(PeriodTrackingApp::add_symptoms(args, context))
            }
        )
        .with_command_async(
            Command::new("share")
                .arg(Arg::new("id").required(true).long("id").short('i'))
                .arg(
                    Arg::new("readers")
                        .required(false)
                        .long("readers")
                        .short('r')
                        .action(ArgAction::Append),
                )
                .arg(
                    Arg::new("writers")
                        .required(false)
                        .long("writers")
                        .short('w')
                        .action(ArgAction::Append),
                ),
            |args, context| {
                Box::pin(PeriodTrackingApp::share(args, context))
            },
        );

    repl.run_async().await
}
