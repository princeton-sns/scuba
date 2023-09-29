use chrono::DateTime;
use noise_kv::client::NoiseKVClient;
use noise_kv::data::NoiseData;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/*
 * Calendar
 * - allows clients to book appointments with providers given the
 *   provider's availability
 * - providers share their availability with all clients
 * - appointments are private to provider and the client whom the
 *   appointment is with, but update the provider's overall
 *   availability, visible to their other clients
 * - providers can also block off times on their end without needing an
 *   appointment to be scheduled, e.g. for lunch breaks
 * - the same device can act as both a client and provider
 * - TODO chat functionality?
 * - TODO what consistency model is most appropriate?
 */

// TODO use the struct name as the type/prefix instead
// https://users.rust-lang.org/t/how-can-i-convert-a-struct-name-to-a-string/66724/8
// or
// #[serde(skip_serializing_if = "path")] on all fields (still cumbersome),
// calling simple function w bool if only want struct name
const ROLES_PREFIX: &str = "roles";
const APPT_PREFIX: &str = "appointment";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppointmentInfo {
    date_time: u32,
    // Rust refinement types?
    duration: u32,
    client_notes: String,
}

impl AppointmentInfo {
    fn new(
        date_time: u32,
        duration: u32,
        client_notes: String,
    ) -> AppointmentInfo {
        AppointmentInfo {
            date_time,
            duration,
            client_notes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Durations {
    durations: Vec<u32>,
}

// if no duration specified, default is 50 minutes
const DEFAULT_DUR: u32 = 50;

impl Durations {
    fn new(durations_opt: Option<Vec<u32>>) -> Self {
        match durations_opt {
            Some(durations) => Durations {
                durations: durations,
            },
            None => {
                let mut durations = Vec::<u32>::new();
                durations.push(DEFAULT_DUR);
                Durations {
                    durations: durations,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Blocked {}

impl Blocked {
    fn new() -> Self {
        Blocked {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Provider {
    // TODO id? group?
    blocked_times: Blocked,
    durations: Durations,
    appointments: Vec<AppointmentInfo>,
}

impl Provider {
    fn new(durations: Option<Vec<u32>>) -> Self {
        Provider {
            blocked_times: Blocked::new(),
            durations: Durations::new(durations),
            appointments: Vec::<AppointmentInfo>::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Client {
    // TODO id? group?
    appointments: Vec<AppointmentInfo>,
}

impl Client {
    fn new() -> Self {
        Client {
            appointments: Vec::<AppointmentInfo>::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Roles {
    provider: Option<Provider>,
    client: Option<Client>,
}

impl Roles {
    fn new() -> Self {
        Roles {
            provider: None,
            client: None,
        }
    }
}

#[derive(Clone)]
struct CalendarApp {
    client: NoiseKVClient,
}

impl CalendarApp {
    pub async fn new() -> CalendarApp {
        let client = NoiseKVClient::new(None, None, false, None, None).await;
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

    pub async fn init_new_device(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();

        let roles_id = ROLES_PREFIX.to_owned();
        let roles_data = Roles::new();
        let json_string = serde_json::to_string(&roles_data).unwrap();

        match context
            .client
            .set_data(
                roles_id.clone(),
                ROLES_PREFIX.to_string(),
                json_string,
                None,
            )
            .await
        {
            Ok(_) => Ok(Some(String::from("Standalone device created."))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create device: {}",
                err.to_string()
            )))),
        }
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

    pub async fn init_role(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut init_provider = false;
        let mut init_client = false;
        if args.get_flag("provider") {
            init_provider = true;
        }
        if args.get_flag("client") {
            init_client = true;
        }

        if !init_client && !init_provider {
            return Ok(Some(String::from("No roles to init.")));
        }

        // get roles
        let roles_id = ROLES_PREFIX.to_owned();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let roles_opt = data_store_guard.get_data(&roles_id);

        match roles_opt {
            Some(roles_str) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_str.data_val()).unwrap();

                if init_provider {
                    match args.get_many::<String>("durations") {
                        Some(arg_durations) => {
                            let durations = arg_durations
                                .map(|s| s.parse::<u32>().unwrap())
                                .collect::<Vec<u32>>();
                            roles.provider =
                                Some(Provider::new(Some(durations)));
                        }
                        None => roles.provider = Some(Provider::new(None)),
                    }
                }

                if init_client {
                    roles.client = Some(Client::new());
                }

                let json_string = serde_json::to_string(&roles).unwrap();

                match context
                    .client
                    .set_data(
                        roles_id.clone(),
                        ROLES_PREFIX.to_string(),
                        json_string,
                        None,
                    )
                    .await
                {
                    Ok(_) => Ok(Some(String::from("Created role(s)."))),
                    Err(err) => Ok(Some(String::from(format!(
                        "Error creating role(s): {}",
                        err.to_string()
                    )))),
                }
            }
            None => Ok(Some(String::from("Roles do not exist."))),
        }
    }

    // Called by either client or provider
    pub fn get_appointment(
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
                let val: AppointmentInfo =
                    serde_json::from_str(val_str.data_val()).unwrap();
                Ok(Some(String::from(format!("{:?}", val))))
            }
            None => Ok(Some(String::from(format!(
                "Appointment with id {} does not exist.",
                id,
            )))),
        }
    }

    // TODO
    // Called by client (see all appointments with one provider)
    pub fn get_provider_appointments(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from("TBD")))
    }

    // TODO
    // Called by provider (see all appointments with one client)
    pub fn get_client_appointments(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from("TBD")))
    }

    // TODO
    // Called by client
    pub fn view_availability(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        // TODO provider_id

        Ok(Some(String::from("TBD")))
    }

    // TODO
    // Called by client
    pub async fn schedule_appointment(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        /*
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

        let pass_info = AppointmentInfo::new(app_name, url, username, password);

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
        */
        Ok(Some(String::from("TBD")))
    }

    // TODO
    // Called by client
    pub async fn edit_appointment(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        /*
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

                let mut old_val: AppointmentInfo =
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
        */
        Ok(Some(String::from("TBD")))
    }

    // FIXME No sharing in this way, shares on appointment creation
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
    let app = Arc::new(CalendarApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Calendar App")
        .with_version("v0.1.0")
        .with_description("Noise calendar app")
        .with_command_async(Command::new("init_new_device"), |_, context| {
            Box::pin(CalendarApp::init_new_device(context))
        })
        .with_command_async(
            Command::new("init_linked_device")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(CalendarApp::init_linked_device(args, context))
            },
        )
        .with_command(Command::new("check_device"), CalendarApp::check_device)
        .with_command(Command::new("get_name"), CalendarApp::get_name)
        .with_command(Command::new("get_idkey"), CalendarApp::get_idkey)
        .with_command(
            Command::new("get_contacts").about("broken - don't use"),
            CalendarApp::get_contacts,
        )
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(CalendarApp::add_contact(args, context)),
        )
        .with_command(
            Command::new("get_linked_devices"),
            CalendarApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), CalendarApp::get_data)
        .with_command(Command::new("get_perms"), CalendarApp::get_perms)
        .with_command(Command::new("get_groups"), CalendarApp::get_groups)
        .with_command_async(
            Command::new("init_role")
                .arg(
                    Arg::new("provider")
                        .required(false)
                        .action(ArgAction::SetTrue)
                        .long("provider")
                        .short('p')
                        .help("Init provider role"),
                )
                .arg(
                    Arg::new("durations")
                        .required(false)
                        .action(ArgAction::Append)
                        .long("durations")
                        .short('d')
                        .help("Set valid appointment duration options (provider only)"),
                )
                .arg(
                    Arg::new("blocked")
                        .required(false)
                        .action(ArgAction::Append)
                        .long("blocked")
                        .short('b')
                        .help("Set blocked-off times (provider only)"),
                )
                .arg(
                    Arg::new("client")
                        .required(false)
                        .action(ArgAction::SetTrue)
                        .long("client")
                        .short('c')
                        .help("Init client role"),
                ),
            |args, context| {
                Box::pin(CalendarApp::init_role(args, context))
            },
        )
        .with_command(
            Command::new("get_appointment").arg(Arg::new("id").required(true)),
            CalendarApp::get_appointment,
        )
        .with_command(
            Command::new("get_provider_appointments").arg(
                Arg::new("provider_id")
                    .required(true)
                    .long("provider_id")
                    .short('p'),
            ),
            CalendarApp::get_provider_appointments,
        )
        .with_command(
            Command::new("get_client_appointments").arg(
                Arg::new("client_id")
                    .required(true)
                    .long("client_id")
                    .short('c'),
            ),
            CalendarApp::get_client_appointments,
        )
        .with_command(
            Command::new("view_availability").arg(
                Arg::new("provider_id")
                    .required(true)
                    .long("provider_id")
                    .short('p'),
            ),
            CalendarApp::view_availability,
        )
        .with_command_async(
            Command::new("schedule_appointment"),
            // FIXME
            |args, context| {
                Box::pin(CalendarApp::schedule_appointment(args, context))
            },
        )
        .with_command_async(
            Command::new("edit_appointment")
                // FIXME
                .arg(Arg::new("id").required(true).long("id").short('i')),
            |args, context| {
                Box::pin(CalendarApp::edit_appointment(args, context))
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
            |args, context| Box::pin(CalendarApp::share(args, context)),
        );

    repl.run_async().await
}
