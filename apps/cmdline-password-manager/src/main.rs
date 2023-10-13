use passwords::PasswordGenerator;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use sequential_noise_kv::client::NoiseKVClient;
use sequential_noise_kv::data::NoiseData;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/*
 * Password Manager
 * - [x] automatically generates passwords
 * - [ ] login/logout functionality
 * - [x] copy/paste passwords
 * - [x] stores encrypted passwords for any account across devices
 * - [x] allows users to easily access stored passwords
 * - [x] safely shares passwords/credentials across multiple users
 * - [ ] linearizability (real-time constraints for password-updates in
 *   groups of multiple users)
 * - [ ] 2FA
 *   - [ ] TOTP
 *   - [ ] HOTP
 * - [x] per-application password invariants
 *   - e.g. min characters, allowed special characters, etc
 */

// TODO use the struct name as the type/prefix instead
// https://users.rust-lang.org/t/how-can-i-convert-a-struct-name-to-a-string/66724/8
// or
// #[serde(skip_serializing_if = "path")] on all fields (still cumbersome),
// calling simple function w bool if only want struct name
//const PASS_PREFIX: &str = "pass";
const CONFIG_PREFIX: &str = "config";
const TOTP_PREFIX: &str = "totp_pass";
const HOTP_PREFIX: &str = "hotp_pass";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswordTOTP {
    app_name: String,
    url: Option<String>,
    username: String,
    password: String,
    otp_secret: String,
}

impl PasswordTOTP {
    fn new(
        app_name: String,
        url: Option<String>,
        username: String,
        password: String,
        otp_secret: String,
    ) -> Self {
        PasswordTOTP {
            app_name,
            url,
            username,
            password,
            otp_secret,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswordHOTP {
    app_name: String,
    url: Option<String>,
    username: String,
    password: String,
    otp_secret: String,
    otp_counter: u64,
}

impl PasswordHOTP {
    fn new(
        app_name: String,
        url: Option<String>,
        username: String,
        password: String,
        otp_secret: String,
        otp_counter: u64,
    ) -> Self {
        PasswordHOTP {
            app_name,
            url,
            username,
            password,
            otp_secret,
            otp_counter,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswordConfig {
    app_name: String,
    length: usize,
    numbers: bool,
    lowercase_letters: bool,
    uppercase_letters: bool,
    symbols: bool,
}

impl PasswordConfig {
    fn new(
        app_name: String,
        length: usize,
        numbers: bool,
        lowercase_letters: bool,
        uppercase_letters: bool,
        symbols: bool,
    ) -> Self {
        PasswordConfig {
            app_name,
            length,
            numbers,
            lowercase_letters,
            uppercase_letters,
            symbols,
        }
    }
}

#[derive(Clone)]
struct PasswordManager {
    client: NoiseKVClient,
    // iterator would be more efficient but compiler isn't finding this struct
    //pgi: passwords::PasswordGeneratorIter,
    //pgi: PasswordGenerator,
}

impl PasswordManager {
    pub async fn new() -> PasswordManager {
        let client = NoiseKVClient::new(None, None, false, None, None).await;
        Self { client }
    }

    fn new_prefixed_id(prefix: &String) -> String {
        let mut id: String = prefix.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        id
    }

    pub fn config_app_password(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let app_name = args.get_one::<String>("app_name").unwrap().to_string();
        let length = args.get_one::<String>("length").unwrap().to_string();
        let numbers = args.get_one::<String>("numbers").unwrap().to_string();
        let lowercase_letters = args.get_one::<String>("lc").unwrap().to_string();
        let uppercase_letters = args.get_one::<String>("uc").unwrap().to_string();
        let symbols = args.get_one::<String>("symbols").unwrap().to_string();

        let config = PasswordConfig::new(app_name, length, numbers, lowercase_letters, uppercase_letters, symbols);

        let mut id: String = CONFIG_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(app_name);
        let json_string = serde_json::to_string(&config).unwrap();

        match context
            .client
            .set_data(id.clone(), CONFIG_PREFIX.to_string(), json_string, None)
            .await
        {
            Ok(_) => {
                Ok(Some(String::from(format!("Added config with id {}", id))))
            }
            Err(err) => Ok(Some(String::from(format!(
                "Error adding config: {}",
                err.to_string()
            )))),
        }
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
        let mut hotp = false;
        let mut totp = false;
        if args.get_flag("hotp") {
            hotp = true;
        }
        if args.get_flag("totp") {
            totp = true;
        }
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let val_opt = data_store_guard.get_data(&id);

        match val_opt {
            Some(val_str) => {
                if hotp {
                    let val: PasswordHOTP =
                        serde_json::from_str(val_str.data_val()).unwrap();
                    Ok(Some(String::from(format!("{}", val.password))))
                } else {
                    let val: PasswordTOTP =
                        serde_json::from_str(val_str.data_val()).unwrap();
                    Ok(Some(String::from(format!("{}", val.password))))
                }
            }
            None => Ok(Some(String::from(format!(
                "Password with id {} does not exist.",
                id,
            )))),
        }
    }

    fn gen_password_from_config(&self, app_name: &String) -> String {
        let mut config_id: String = CONFIG_PREFIX.to_owned();
        config_id.push_str("/");
        config_id.push_str(app_name);
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        // breaks if no config previously set
        let config = data_store_guard.get_data(&config_id).unwrap().clone();
        let mut config_obj: PasswordConfig =
            serde_json::from_str(config.data_val()).unwrap();
        // this is horrible for perf
        let pgi = PasswordGenerator::new()
            .length(config_obj.length)
            .numbers(config_obj.numbers)
            .lowercase_letters(config_obj.lowercase_letters)
            .uppercase_letters(config_obj.uppercase_letters)
            .symbols(config_obj.symbols);
        pgi.try_iter().unwrap().next().unwrap()
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
        let mut hotp = false;
        let mut totp = false;
        if args.get_flag("hotp") {
            hotp = true;
        }
        if args.get_flag("totp") {
            totp = true;
        }
        let url;
        match args.get_one::<String>("url") {
            Some(arg_url) => url = Some(arg_url.to_string()),
            None => url = None,
        }
        let username = args.get_one::<String>("username").unwrap().to_string();
        let password;
        match args.get_one::<String>("password") {
            Some(arg_pass) => password = arg_pass.to_string(),
            None => password = context.gen_password_from_config(app_name);
        }

        if hotp {
            let pass_info = PasswordHOTP::new(app_name, url, username, password, otp_secret);
            let mut id = Self::new_prefixed_id(HOTP_PREFIX);
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
        } else {
            let pass_info = PasswordTOTP::new(app_name, url, username, password, otp_secret);
            let mut id = Self::new_prefixed_id(HOTP_PREFIX);
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
    }

    fn hash(first: String, second: String) -> {
        use sha2::Digest;

        let mut hasher = sha2::Sha256::new();
        hasher.update(b"first");
        hasher.update(first);
        hasher.update(b"second");
        hasher.update(second);

        let hash = hasher.finalize();
        base64ct::Base64::encode_string(&hash)
    }

    pub async fn get_otp(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let id = args.get_one::<String>("id").unwrap().to_string();
        let mut hotp = false;
        let mut totp = false;
        if args.get_flag("hotp") {
            hotp = true;
        }
        if args.get_flag("totp") {
            totp = true;
        }
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let val_opt = data_store_guard.get_data(&id);

        match val_opt {
            Some(val_str) => {
                if hotp {
                    let val: PasswordHOTP =
                        serde_json::from_str(val_str.data_val()).unwrap();
                    let hash_res = hash(val.otp_secret, val.otp_counter);
                    val.otp_counter += 1;
                    let hotp_json = serde_json::to_string(&val).unwrap();
                    match context
                        .client
                        .set_data(
                            id.clone(),
                            HOTP_PREFIX.to_string(),
                            hotp_json,
                            None,
                        )
                        .await
                    {
                        Ok(_) => Ok(Some(String::from(format!("OTP: {}", hash_res)))),
                        Err(err) => Ok(Some(String::from(format!(
                            "Could not update counter for HOTP: {}", err.to_string()
                        )))),
                    }
                } else {
                    let val: PasswordTOTP =
                        serde_json::from_str(val_str.data_val()).unwrap();
                    let cur_time = chrono::offset::Utc::now();
                    let hash_res = hash(val.otp_secret, serde_json::to_string(&cur_time).unwrap());
                    Ok(Some(String::from(format!("OTP: {}", hash_res))))
                }
            }
            None => Ok(Some(String::from(format!(
                "Password with id {} does not exist.",
                id,
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
        let mut hotp = false;
        let mut totp = false;
        if args.get_flag("hotp") {
            hotp = true;
        }
        if args.get_flag("totp") {
            totp = true;
        }
        let app_name = args.get_one::<String>("app_name").unwrap().to_string();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let val_opt = data_store_guard.get_data(&id);

        match val_opt {
            Some(val_str) => {
                let password;
                match args.get_one::<String>("password") {
                    Some(arg_pass) => password = arg_pass.to_string(),
                    None => password = context.gen_password_from_config(app_name),
                }
                
                if hotp {
                    let mut old_val: PasswordHOTP =
                        serde_json::from_str(val_str.data_val()).unwrap();
                    old_val.password = password;
                    let json_string = serde_json::to_string(&old_val).unwrap();

                    match context
                        .client
                        .set_data(
                            id.clone(),
                            HOTP_PREFIX.to_string(),
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
                } else {
                    let mut old_val: PasswordTOTP =
                        serde_json::from_str(val_str.data_val()).unwrap();
                    old_val.password = password;
                    let json_string = serde_json::to_string(&old_val).unwrap();

                    match context
                        .client
                        .set_data(
                            id.clone(),
                            TOTP_PREFIX.to_string(),
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
        //.with_command(
        //    Command::new("get_contacts").about("broken - don't use"),
        //    PasswordManager::get_contacts,
        //)
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
            Command::new("get_password")
                .arg(Arg::new("id").required(true))
                .arg(Arg::new("hotp").action(ArgAction::SetTrue).required(false))
                .arg(Arg::new("totp").action(ArgAction::SetTrue).required(false))
                .about("use either hotp or totp depending on the password id")
            ,
            PasswordManager::get_password,
        )
        .with_command_async(
            Command::new("config_app_password")
                .arg(
                    Arg::new("app_name")
                        .required(true)
                        .long("app_name")
                        .short('a'),
                )
                .arg(Arg::new("length").required(true))
                .arg(Arg::new("numbers").required(true))
                .arg(Arg::new("lc").required(true))
                .arg(Arg::new("uc").required(true))
                .arg(Arg::new("symbols").required(true)),
            |args, context| {
                Box::pin(PasswordManager::config_app_password(args, context))
            }
        )
        .with_command_async(
            Command::new("add_password")
                .arg(
                    Arg::new("app_name")
                        .required(true)
                        .long("app_name")
                        .short('a'),
                )
                .arg(Arg::new("hotp").action(ArgAction::SetTrue).required(false))
                .arg(Arg::new("totp").action(ArgAction::SetTrue).required(false))
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
        .with_command(
            Command::new("get_otp")
                .arg(Arg::new("id").required(true))
                .arg(Arg::new("hotp").action(ArgAction::SetTrue).required(false))
                .arg(Arg::new("totp").action(ArgAction::SetTrue).required(false))
                .about("use either hotp or totp depending on the password id")
            ,
            PasswordManager::get_otp,
        )
        .with_command_async(
            Command::new("update_password")
                .arg(Arg::new("id").required(true).long("id").short('i'))
                .arg(Arg::new("app_name").required(true).short('a'))
                .arg(Arg::new("hotp").action(ArgAction::SetTrue).required(false))
                .arg(Arg::new("totp").action(ArgAction::SetTrue).required(false))
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
