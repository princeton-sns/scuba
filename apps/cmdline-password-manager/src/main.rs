use passwords::PasswordGenerator;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use single_key_dal::client::NoiseKVClient;
use single_key_dal::data::NoiseData;
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
 * - 2FA
 *   - [x] TOTP
 *   - [x] HOTP
 * - [x] per-application password invariants
 *   - e.g. min characters, allowed special characters, etc
 */

// TODO use the struct name as the type/prefix instead
// https://users.rust-lang.org/t/how-can-i-convert-a-struct-name-to-a-string/66724/8
// or
// #[serde(skip_serializing_if = "path")] on all fields (still cumbersome),
// calling simple function w bool if only want struct name
const CONFIG_PREFIX: &str = "config";
const TOTP_PREFIX: &str = "totp_pass";
const HOTP_PREFIX: &str = "hotp_pass";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswordTOTP {
    app_name: String,
    username: String,
    password: String,
    otp_secret: String,
}

impl PasswordTOTP {
    fn new(
        app_name: String,
        username: String,
        password: String,
        otp_secret: String,
    ) -> Self {
        PasswordTOTP {
            app_name,
            username,
            password,
            otp_secret,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswordHOTP {
    app_name: String,
    username: String,
    password: String,
    otp_secret: String,
    otp_counter: u64,
}

impl PasswordHOTP {
    fn new(
        app_name: String,
        username: String,
        password: String,
        otp_secret: String,
    ) -> Self {
        PasswordHOTP {
            app_name,
            username,
            password,
            otp_secret,
            otp_counter: 0,
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
}

impl PasswordManager {
    pub async fn new() -> PasswordManager {
        let client = NoiseKVClient::new(
            None,
            None,
            false,
            Some("passmanager.txt"),
            None,
            None,
            // linearizability
            true,
            true,
            false,
        )
        .await;
        Self { client }
    }

    fn new_prefixed_id(prefix: &String) -> String {
        let mut id: String = prefix.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        id
    }

    pub async fn config_app_password(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let app_name = args.get_one::<String>("app_name").unwrap().to_string();
        let length_str = args.get_one::<String>("length").unwrap().to_string();
        let length: usize = length_str.parse().unwrap();

        let config = PasswordConfig::new(
            app_name.clone(),
            length,
            args.get_flag("numbers"),
            args.get_flag("lc"),
            args.get_flag("uc"),
            args.get_flag("symbols"),
        );

        let mut id: String = CONFIG_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(&app_name);
        let json_string = serde_json::to_string(&config).unwrap();

        match context
            .client
            .set_data(id.clone(), CONFIG_PREFIX.to_string(), json_string, None, None)
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

    pub async fn init_new_device(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device().await;
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

    pub async fn get_linked_devices(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let linked_devices = context.client.get_linked_devices().await.unwrap();
        Ok(Some(itertools::join(linked_devices, "\n")))
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

    pub async fn get_data(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        if let Some(id) = args.get_one::<String>("id") {
            match context.client.get_data(id).await {
                Ok(Some(data)) => Ok(Some(String::from(format!("{}", data)))),
                Ok(None) => Ok(Some(String::from(format!(
                    "Data with id {} does not exist",
                    id
                )))),
                Err(err) => Ok(Some(String::from(format!(
                    "Could not get data: {}",
                    err.to_string()
                )))),
            }
        } else {
            let data = context.client.get_all_data().await.unwrap();
            Ok(Some(itertools::join(data, "\n")))
        }
    }

    pub async fn get_perms(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        if let Some(id) = args.get_one::<String>("id") {
            match context.client.get_perm(id).await {
                Ok(Some(perm)) => Ok(Some(String::from(format!("{}", perm)))),
                Ok(None) => Ok(Some(String::from(format!(
                    "Perm with id {} does not exist",
                    id
                )))),
                Err(err) => Ok(Some(String::from(format!(
                    "Could not get perm: {}",
                    err.to_string()
                )))),
            }
        } else {
            let perms = context.client.get_all_perms().await.unwrap();
            Ok(Some(itertools::join(perms, "\n")))
        }
    }

    pub async fn get_groups(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        if let Some(id) = args.get_one::<String>("id") {
            match context.client.get_group(id).await {
                Ok(Some(group)) => Ok(Some(String::from(format!("{}", group)))),
                Ok(None) => Ok(Some(String::from(format!(
                    "Group with id {} does not exist",
                    id
                )))),
                Err(err) => Ok(Some(String::from(format!(
                    "Could not get group: {}",
                    err.to_string()
                )))),
            }
        } else {
            let groups = context.client.get_all_groups().await.unwrap();
            Ok(Some(itertools::join(groups, "\n")))
        }
    }

    pub async fn get_password(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("id").unwrap().to_string();

        match context.client.get_data(&id).await {
            Ok(Some(val_obj)) => {
                if val_obj.data_id().starts_with(HOTP_PREFIX) {
                    let val: PasswordHOTP =
                        serde_json::from_str(val_obj.data_val()).unwrap();
                    Ok(Some(String::from(format!("{}", val.password))))
                } else {
                    let val: PasswordTOTP =
                        serde_json::from_str(val_obj.data_val()).unwrap();
                    Ok(Some(String::from(format!("{}", val.password))))
                }
            }
            Ok(None) => Ok(Some(String::from(format!(
                "Password with id {} does not exist.",
                id,
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not get data: {}",
                err.to_string()
            )))),
        }
    }

    async fn gen_password_from_config(&self, app_name: &String) -> String {
        let mut config_id: String = CONFIG_PREFIX.to_owned();
        config_id.push_str("/");
        config_id.push_str(app_name);
        // breaks if no config previously set FIXME
        let config_opt = self.client.get_data(&config_id).await;
        let config = config_opt.unwrap().unwrap().clone();
        let config_obj: PasswordConfig =
            serde_json::from_str(config.data_val()).unwrap();
        // this is horrible for perf?
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
        let t = args.get_one::<String>("type").unwrap().to_string();
        if t != "hotp" && t != "totp" {
            return Ok(Some(String::from(format!("Type argument must either be <hotp> or <totp>; argument was {}", t))));
        }
        let mut hotp = false;
        if t == "hotp" {
            hotp = true;
        }
        let username = args.get_one::<String>("username").unwrap().to_string();
        let otp_secret = args.get_one::<String>("secret").unwrap().to_string();
        let password;
        match args.get_one::<String>("password") {
            Some(arg_pass) => password = arg_pass.to_string(),
            None => {
                password = context.gen_password_from_config(&app_name).await
            }
        }

        if hotp {
            let pass_info =
                PasswordHOTP::new(app_name, username, password, otp_secret);
            let id = Self::new_prefixed_id(&HOTP_PREFIX.to_string());
            let json_string = serde_json::to_string(&pass_info).unwrap();
            match context
                .client
                .set_data(
                    id.clone(),
                    HOTP_PREFIX.to_string(),
                    json_string,
                    None,
                    None,
                )
                .await
            {
                Ok(_) => Ok(Some(String::from(format!(
                    "Added password with id {}",
                    id
                )))),
                Err(err) => Ok(Some(String::from(format!(
                    "Error adding password: {}",
                    err.to_string()
                )))),
            }
        } else {
            let pass_info =
                PasswordTOTP::new(app_name, username, password, otp_secret);
            let id = Self::new_prefixed_id(&TOTP_PREFIX.to_string());
            let json_string = serde_json::to_string(&pass_info).unwrap();
            match context
                .client
                .set_data(
                    id.clone(),
                    TOTP_PREFIX.to_string(),
                    json_string,
                    None,
                    None,
                )
                .await
            {
                Ok(_) => Ok(Some(String::from(format!(
                    "Added password with id {}",
                    id
                )))),
                Err(err) => Ok(Some(String::from(format!(
                    "Error adding password: {}",
                    err.to_string()
                )))),
            }
        }
    }

    fn hash(first: String, second: String) -> String {
        use base64ct::Encoding;
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
        match context.client.get_data(&id).await {
            Ok(Some(val_obj)) => {
                if val_obj.data_id().starts_with(HOTP_PREFIX) {
                    let mut val: PasswordHOTP =
                        serde_json::from_str(val_obj.data_val()).unwrap();
                    let hash_res = Self::hash(
                        val.otp_secret.clone(),
                        val.otp_counter.to_string(),
                    );
                    val.otp_counter += 1;
                    let hotp_json = serde_json::to_string(&val).unwrap();
                    match context
                        .client
                        .set_data(
                            id.clone(),
                            HOTP_PREFIX.to_string(),
                            hotp_json,
                            None,
                            None,
                        )
                        .await
                    {
                        Ok(_) => {
                            Ok(Some(String::from(format!("OTP: {}", hash_res))))
                        }
                        Err(err) => Ok(Some(String::from(format!(
                            "Could not update counter for HOTP: {}",
                            err.to_string()
                        )))),
                    }
                } else {
                    let val: PasswordTOTP =
                        serde_json::from_str(val_obj.data_val()).unwrap();
                    let cur_time = chrono::offset::Utc::now();
                    let hash_res = Self::hash(
                        val.otp_secret,
                        serde_json::to_string(&cur_time).unwrap(),
                    );
                    Ok(Some(String::from(format!("OTP: {}", hash_res))))
                }
            }
            Ok(None) => Ok(Some(String::from(format!(
                "Password with id {} does not exist.",
                id,
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Cannot read password: {}",
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
        let app_name = args.get_one::<String>("app_name").unwrap().to_string();

        match context.client.get_data(&id).await {
            Ok(Some(val_obj)) => {
                let password;
                match args.get_one::<String>("password") {
                    Some(arg_pass) => password = arg_pass.to_string(),
                    None => {
                        password =
                            context.gen_password_from_config(&app_name).await
                    }
                }

                if val_obj.data_id().starts_with(HOTP_PREFIX) {
                    let mut old_val: PasswordHOTP =
                        serde_json::from_str(val_obj.data_val()).unwrap();
                    old_val.password = password;
                    let json_string = serde_json::to_string(&old_val).unwrap();

                    match context
                        .client
                        .set_data(
                            id.clone(),
                            HOTP_PREFIX.to_string(),
                            json_string,
                            None,
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
                        serde_json::from_str(val_obj.data_val()).unwrap();
                    old_val.password = password;
                    let json_string = serde_json::to_string(&old_val).unwrap();

                    match context
                        .client
                        .set_data(
                            id.clone(),
                            TOTP_PREFIX.to_string(),
                            json_string,
                            None,
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
            Ok(None) => Ok(Some(String::from(format!(
                "Password with id {} does not exist.",
                id,
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Cannot read password: {}",
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

        let config_id =
            args.get_one::<String>("config_id").unwrap().to_string();
        let pass_id = args.get_one::<String>("pass_id").unwrap().to_string();

        if let Some(arg_readers) = args.get_many::<String>("readers") {
            let readers = arg_readers.collect::<Vec<&String>>();
            let res = context
                .client
                .add_readers(pass_id.clone(), readers.clone())
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
            // share config obj
            let mut res = context
                .client
                .add_writers(config_id.clone(), writers.clone())
                .await;
            if res.is_err() {
                return Ok(Some(String::from(format!(
                    "Error adding writers to config obj: {}",
                    res.err().unwrap().to_string()
                ))));
            }
            // share pass obj
            res = context
                .client
                .add_writers(pass_id.clone(), writers.clone())
                .await;
            if res.is_err() {
                return Ok(Some(String::from(format!(
                    "Error adding writers to password obj: {}",
                    res.err().unwrap().to_string()
                ))));
            }
        }

        Ok(Some(String::from(format!(
            "Successfully shared datum {}",
            pass_id
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
        .with_command_async(
            Command::new("init_new_device"),
            |_, context| {
                Box::pin(PasswordManager::init_new_device(context))
            },
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
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(PasswordManager::add_contact(args, context))
            },
        )
        .with_command_async(
            Command::new("get_linked_devices"),
            |_, context| {
                Box::pin(PasswordManager::get_linked_devices(context))
            },
        )
        .with_command_async(
            Command::new("get_data").arg(Arg::new("id").required(false)),
            |args, context| Box::pin(PasswordManager::get_data(args, context)),
        )
        .with_command_async(
            Command::new("get_perms").arg(Arg::new("id").required(true)),
            |args, context| Box::pin(PasswordManager::get_perms(args, context)),
        )
        .with_command_async(
            Command::new("get_groups").arg(Arg::new("id").required(true)),
            |args, context| {
                Box::pin(PasswordManager::get_groups(args, context))
            },
        )
        .with_command_async(
            Command::new("get_password").arg(Arg::new("id").required(true)),
            |args, context| {
                Box::pin(PasswordManager::get_password(args, context))
            },
        )
        .with_command_async(
            Command::new("config_app_password")
                .arg(
                    Arg::new("app_name")
                        .required(true)
                        .long("app_name")
                        .short('a'),
                )
                .arg(
                    Arg::new("length").required(true).long("length").short('l'),
                )
                .arg(
                    Arg::new("numbers")
                        .action(ArgAction::SetTrue)
                        .required(false)
                        .long("numbers")
                        .short('n'),
                )
                .arg(
                    Arg::new("lc")
                        .action(ArgAction::SetTrue)
                        .required(false)
                        .long("lowercase")
                        .long("lc"),
                )
                .arg(
                    Arg::new("uc")
                        .action(ArgAction::SetTrue)
                        .required(false)
                        .long("uppercase")
                        .long("uc"),
                )
                .arg(
                    Arg::new("symbols")
                        .action(ArgAction::SetTrue)
                        .required(false)
                        .long("symbols")
                        .short('s'),
                ),
            |args, context| {
                Box::pin(PasswordManager::config_app_password(args, context))
            },
        )
        .with_command_async(
            Command::new("add_password")
                .arg(
                    Arg::new("app_name")
                        .required(true)
                        .long("app_name")
                        .short('a'),
                )
                .arg(
                    Arg::new("secret").required(true).long("secret").short('s'),
                )
                .arg(Arg::new("type").required(true).short('t'))
                .about("use either hotp or totp depending on the password id")
                .arg(
                    Arg::new("username")
                        .required(true)
                        .long("username")
                        .short('u'),
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
            Command::new("get_otp").arg(Arg::new("id").required(true)),
            |args, context| Box::pin(PasswordManager::get_otp(args, context)),
        )
        .with_command_async(
            Command::new("update_password")
                .arg(Arg::new("id").required(true).long("id").short('i'))
                .arg(Arg::new("app_name").required(true).short('a'))
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
                .arg(
                    Arg::new("config_id")
                        .required(true)
                        .long("config_id")
                        .short('c'),
                )
                .arg(
                    Arg::new("pass_id")
                        .required(true)
                        .long("pass_id")
                        .short('p'),
                )
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
