use noise_kv::client::NoiseKVClient;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use std::sync::Arc;
use uuid::Uuid;

const FLOW_PREFIX: &str = "flow";
const SYMPTOMS_PREFIX: &str = "symptom";
const FLOW_VAL: &str = r#"{ "flow": medium }"#;
const SYMPTOMS_VAL: &str = r#"{ "symptoms": [bloating] }"#;

#[derive(Clone)]
struct PeriodTrackingApp {
    client: NoiseKVClient,
}

impl PeriodTrackingApp {
    pub async fn new() -> PeriodTrackingApp {
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

    pub fn get_flow_state(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("flow_id").unwrap().to_string();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let val_opt = data_store_guard.get_data(&id);

        match val_opt {
            Some(val) => Ok(Some(String::from(format!("{}", val)))),
            None => Ok(Some(String::from(format!(
                "Flow datum with id {} does not exist.",
                id,
            )))),
        }
    }

    pub fn get_symptoms_state(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("symptoms_id").unwrap().to_string();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let val_opt = data_store_guard.get_data(&id);

        match val_opt {
            Some(val) => Ok(Some(String::from(format!("{}", val)))),
            None => Ok(Some(String::from(format!(
                "Symptoms datum with id {} does not exist.",
                id,
            )))),
        }
    }

    pub async fn add_flow(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let mut id: String = FLOW_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        // TODO
        let json_val = FLOW_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), FLOW_PREFIX.to_string(), json_val, None)
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
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let mut id: String = SYMPTOMS_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        // TODO
        let json_val = SYMPTOMS_VAL.to_string();
        match context
            .client
            .set_data(id.clone(), SYMPTOMS_PREFIX.to_string(), json_val, None)
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

    pub async fn add_readers_to_flow(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("flow_id").unwrap().to_string();
        let names = args
            .get_many::<String>("names")
            .unwrap()
            .collect::<Vec<&String>>();
        match context.client.add_readers(id.clone(), names.clone()).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Adding below readers to flow datum with id {}: \n{}",
                id,
                itertools::join(names, "\n")
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Error adding readers to flow datum: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn add_readers_to_symptoms(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("symptoms_id").unwrap().to_string();
        let names = args
            .get_many::<String>("names")
            .unwrap()
            .collect::<Vec<&String>>();
        match context.client.add_readers(id.clone(), names.clone()).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Adding below readers to symptoms datum with id {}: \n{}",
                id,
                itertools::join(names, "\n")
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Error adding readers to symptoms datum: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn add_writers_to_flow(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("flow_id").unwrap().to_string();
        let names = args
            .get_many::<String>("names")
            .unwrap()
            .collect::<Vec<&String>>();
        match context.client.add_writers(id.clone(), names.clone()).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Adding below writers to flow datum with id {}: \n{}",
                id,
                itertools::join(names, "\n")
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Error adding writers to flow datum: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn add_writers_to_symptoms(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("symptoms_id").unwrap().to_string();
        let names = args
            .get_many::<String>("names")
            .unwrap()
            .collect::<Vec<&String>>();
        match context.client.add_writers(id.clone(), names.clone()).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Adding below writers to symptoms datum with id {}: \n{}",
                id,
                itertools::join(names, "\n")
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Error adding writers to symptoms datum: {}",
                err.to_string()
            )))),
        }
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(PeriodTrackingApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("PeriodTracking App")
        .with_version("v0.1.0")
        .with_description("Noise period tracking app")
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
            Command::new("get_flow_state")
                .arg(Arg::new("flow_id").required(true)),
            PeriodTrackingApp::get_flow_state,
        )
        .with_command(
            Command::new("get_symptoms_state")
                .arg(Arg::new("symptoms_id").required(true)),
            PeriodTrackingApp::get_symptoms_state,
        )
        .with_command_async(Command::new("add_flow"), |_, context| {
            Box::pin(PeriodTrackingApp::add_flow(context))
        })
        .with_command_async(Command::new("add_symptoms"), |_, context| {
            Box::pin(PeriodTrackingApp::add_flow(context))
        })
        .with_command_async(
            Command::new("add_readers_to_flow")
                .arg(Arg::new("flow_id").required(true).long("id").short('i'))
                .arg(
                    Arg::new("names")
                        .required(true)
                        .long("name")
                        .short('n')
                        .action(ArgAction::Append),
                ),
            |args, context| {
                Box::pin(PeriodTrackingApp::add_readers_to_flow(args, context))
            },
        )
        .with_command_async(
            Command::new("add_writers_to_flow")
                .arg(Arg::new("flow_id").required(true).long("id").short('i'))
                .arg(
                    Arg::new("names")
                        .required(true)
                        .long("name")
                        .short('n')
                        .action(ArgAction::Append),
                ),
            |args, context| {
                Box::pin(PeriodTrackingApp::add_writers_to_flow(args, context))
            },
        );

    repl.run_async().await
}
