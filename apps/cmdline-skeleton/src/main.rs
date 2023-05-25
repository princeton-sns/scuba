use noise_kv::client::NoiseKVClient;
use noise_kv::data::NoiseData;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use std::sync::Arc;
use uuid::Uuid;

const DATA_PREFIX: &str = "data";
const EMPTY_DATUM: &str = "{}";

#[derive(Clone)]
struct SkeletonApp {
    client: NoiseKVClient,
}

impl SkeletonApp {
    pub async fn new() -> SkeletonApp {
        let client = NoiseKVClient::new(
            None, None, false, None, None,
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

    pub async fn add_datum(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let mut id = DATA_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        let json_string = EMPTY_DATUM.to_string();

        match context
            .client
            .set_data(
                id.clone(),
                DATA_PREFIX.to_string(),
                json_string,
                None,
            )
            .await
        {
            Ok(_) => {
                Ok(Some(String::from(format!("Added datum with id {}", id))))
            }
            Err(err) => Ok(Some(String::from(format!(
                "Error adding datum: {}",
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
    let app = Arc::new(SkeletonApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Skeleton App")
        .with_version("v0.1.0")
        .with_description("Noise skeleton app")
        .with_command(
            Command::new("create_new_device"),
            SkeletonApp::create_new_device,
        )
        .with_command_async(
            Command::new("create_linked_device")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(SkeletonApp::create_linked_device(args, context))
            },
        )
        .with_command(
            Command::new("check_device"),
            SkeletonApp::check_device,
        )
        .with_command(Command::new("get_name"), SkeletonApp::get_name)
        .with_command(Command::new("get_idkey"), SkeletonApp::get_idkey)
        .with_command(
            Command::new("get_contacts"),
            SkeletonApp::get_contacts,
        )
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(SkeletonApp::add_contact(args, context))
            },
        )
        .with_command(
            Command::new("get_linked_devices"),
            SkeletonApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), SkeletonApp::get_data)
        .with_command(Command::new("get_perms"), SkeletonApp::get_perms)
        .with_command(Command::new("get_groups"), SkeletonApp::get_groups)
        .with_command(
            Command::new("get_state")
                .arg(Arg::new("id").required(true)),
            SkeletonApp::get_state,
        )
        .with_command_async(
            Command::new("add_datum"), |_, context| {
                Box::pin(SkeletonApp::add_datum(context))
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
                Box::pin(SkeletonApp::share(args, context))
            },
        );

    repl.run_async().await
}
