use noise_kv::client::NoiseKVClient;
use noise_kv::data::NoiseData;
use passwords::PasswordGenerator;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/*
 * Password Manager
 * - [x] automatically generates strong passwords
 * - [ ] TODO external app interaction
 *   - for now: copy/paste passwords
 * - [ ] login/logout functionality
 * - [x] stores encrypted passwords for any account across devices
 * - [x] allows users to easily access stored passwords
 * - [x] safely shares passwords/credentials across multiple users
 * - [ ] linearizability (real-time constraints for password-updates in
 *   groups of multiple users)
 */

// TODO use the struct name as the type/prefix instead
// https://users.rust-lang.org/t/how-can-i-convert-a-struct-name-to-a-string/66724/8
// or
// #[serde(skip_serializing_if = "path")] on all fields (still cumbersome),
// calling simple function w bool if only want struct name
const PASS_PREFIX: &str = "pass";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswordInfo {
    app_name: String,
    url: Option<String>,
    username: String,
    password: String,
}

impl PasswordInfo {
    fn new(
        app_name: String,
        url: Option<String>,
        username: String,
        password: String,
    ) -> PasswordInfo {
        PasswordInfo {
            app_name,
            url,
            username,
            password,
        }
    }
}

#[derive(Clone)]
struct PasswordManager {
    client: NoiseKVClient,
    // iterator would be more efficient but compiler isn't finding this struct
    //pgi: passwords::PasswordGeneratorIter,
    pgi: PasswordGenerator,
}

impl PasswordManager {
    pub async fn new() -> PasswordManager {
        let client = NoiseKVClient::new(None, None, false, None, None).await;
        // TODO allow configuration
        let pgi = PasswordGenerator::new()
            .length(8)
            .numbers(true)
            .lowercase_letters(true)
            .uppercase_letters(true)
            .symbols(true)
            .spaces(true)
            .exclude_similar_characters(true)
            .strict(true);
        //.try_iter()
        //.unwrap();
        Self { client, pgi }
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

    pub fn init_new_device(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();
        Ok(Some(String::from("Standalone device created.")))
    }

    pub async fn init_linked_device(
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

    pub fn get_password(
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
            Some(val_str) => {
                let val: PasswordInfo =
                    serde_json::from_str(val_str.data_val()).unwrap();
                Ok(Some(String::from(format!("{}", val.password))))
            }
            None => Ok(Some(String::from(format!(
                "Password with id {} does not exist.",
                id,
            )))),
        }
    }

    pub async fn add_password(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let app_name = args.get_one::<String>("app_name").unwrap().to_string();
        let url;
        match args.get_one::<String>("url") {
            Some(arg_url) => url = Some(arg_url.to_string()),
            None => url = None,
        }
        let username = args.get_one::<String>("username").unwrap().to_string();
        let password;
        match args.get_one::<String>("password") {
            Some(arg_pass) => password = arg_pass.to_string(),
            None => {
                // is this horrible for perf?
                password = context.pgi.try_iter().unwrap().next().unwrap();
            }
        }

        let pass_info = PasswordInfo::new(app_name, url, username, password);

        let mut id = PASS_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        let json_string = serde_json::to_string(&pass_info).unwrap();

        match context
            .client
            .set_data(id.clone(), PASS_PREFIX.to_string(), json_string, None)
            .await
        {
            Ok(_) => {
                Ok(Some(String::from(format!("Added password with id {}", id))))
            }
            Err(err) => Ok(Some(String::from(format!(
                "Error adding password: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn update_password(
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
            Some(val_str) => {
                let password;
                match args.get_one::<String>("password") {
                    Some(arg_pass) => password = arg_pass.to_string(),
                    None => {
                        // is this horrible for perf?
                        password =
                            context.pgi.try_iter().unwrap().next().unwrap();
                    }
                }

                let mut old_val: PasswordInfo =
                    serde_json::from_str(val_str.data_val()).unwrap();
                old_val.password = password;
                let json_string = serde_json::to_string(&old_val).unwrap();

                match context
                    .client
                    .set_data(
                        id.clone(),
                        PASS_PREFIX.to_string(),
                        json_string,
                        None,
                    )
                    .await
                {
                    Ok(_) => Ok(Some(String::from(format!(
                        "Updated password with id {}",
                        id
                    )))),
                    Err(err) => Ok(Some(String::from(format!(
                        "Error adding password: {}",
                        err.to_string()
                    )))),
                }
            }
            None => Ok(Some(String::from(format!(
                "Password with id {} does not exist.",
                id,
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
    let app = Arc::new(PasswordManager::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Password Manager App")
        .with_version("v0.1.0")
        .with_description("Noise password manager app")
        .with_command(
            Command::new("init_new_device"),
            PasswordManager::init_new_device,
        )
        .with_command_async(
            Command::new("init_linked_device")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(PasswordManager::init_linked_device(args, context))
            },
        )
        .with_command(
            Command::new("check_device"),
            PasswordManager::check_device,
        )
        .with_command(Command::new("get_name"), PasswordManager::get_name)
        .with_command(Command::new("get_idkey"), PasswordManager::get_idkey)
        .with_command(
            Command::new("get_contacts").about("broken - don't use"),
            PasswordManager::get_contacts,
        )
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(PasswordManager::add_contact(args, context))
            },
        )
        .with_command(
            Command::new("get_linked_devices"),
            PasswordManager::get_linked_devices,
        )
        .with_command(Command::new("get_data"), PasswordManager::get_data)
        .with_command(Command::new("get_perms"), PasswordManager::get_perms)
        .with_command(Command::new("get_groups"), PasswordManager::get_groups)
        .with_command(
            Command::new("get_password").arg(Arg::new("id").required(true)),
            PasswordManager::get_password,
        )
        .with_command_async(
            Command::new("add_password")
                .arg(
                    Arg::new("app_name")
                        .required(true)
                        .long("app_name")
                        .short('a'),
                )
                .arg(Arg::new("url").required(false).long("url").short('u'))
                .arg(
                    Arg::new("username")
                        .required(true)
                        .long("username")
                        .short('n'),
                )
                .arg(
                    Arg::new("password")
                        .required(false)
                        .long("password")
                        .short('p'),
                ),
            |args, context| {
                Box::pin(PasswordManager::add_password(args, context))
            },
        )
        .with_command_async(
            Command::new("update_password")
                .arg(Arg::new("id").required(true).long("id").short('i'))
                .arg(
                    Arg::new("password")
                        .required(false)
                        .long("password")
                        .short('p'),
                ),
            |args, context| {
                Box::pin(PasswordManager::update_password(args, context))
            },
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
            |args, context| Box::pin(PasswordManager::share(args, context)),
        );

    repl.run_async().await
}
