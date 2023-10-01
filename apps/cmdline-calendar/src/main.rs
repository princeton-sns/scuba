use chrono::naive::{NaiveDate, NaiveDateTime, NaiveTime};
use noise_kv::client::NoiseKVClient;
use noise_kv::data::NoiseData;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
const BUSY_PREFIX: &str = "busy";

// appointments can only be made in 60-minute intervals
const DEFAULT_DUR: u32 = 60;

// minutes
//enum Durations {
//    Thirty,
//    Sixty,
//}

/*
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Durations {
    durations: Vec<u32>,
}

impl Durations {
    fn new(durations_opt: Option<Vec<u32>>) -> Self {
        match durations_opt {
            Some(durations) => Durations {
                durations: durations,
            },
            None => Durations {
                durations: vec![DEFAULT_DUR],
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
*/

/*
 * The following structs track information that is private to the
 * current-acting set of linked devices (e.g. user), whether that user
 * is a client, provider, or both.
 */

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Provider {
    appointment_ids: Vec<String>,
    //clients: Vec<String>,
}

impl Provider {
    fn new(durations: Option<Vec<u32>>) -> Self {
        Provider {
            appointment_ids: Vec::<String>::new(),
            //clients: Vec::<String>::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Client {
    appointment_ids: Vec<String>,
    //providers: Vec<String>,
}

impl Client {
    fn new() -> Self {
        Client {
            appointment_ids: Vec::<String>::new(),
            //providers: Vec::<String>::new(),
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

/*
 * The AppointmentInfo struct describes each appointment made and is
 * shared between client and provider
 */

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppointmentInfo {
    date: NaiveDate,
    time: NaiveTime,
    duration_min: u32,
    client_notes: Option<String>,
    pending: bool,
}

impl AppointmentInfo {
    fn new(
        date: NaiveDate,
        time: NaiveTime,
        client_notes: Option<String>,
    ) -> AppointmentInfo {
        AppointmentInfo {
            date,
            time,
            duration_min: DEFAULT_DUR,
            client_notes,
            pending: true,
        }
    }
}

/*
 * The Busy struct info is shared by providers with their clients.
 * It obfuscates all appointment details or blocked slots and simply
 * shows them all as "busy".
 */

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Busy {
    taken_slots: HashMap<NaiveDateTime, u32>,
}

impl Busy {
    fn new() -> Self {
        Busy {
            taken_slots: HashMap::new(),
        }
    }
}

/*
 * Application logic below.
 */

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

    /*
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
    */

    // Called by provider only; upon contact addition, provider shares busy
    // object with client
    pub async fn add_contact(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        // TODO check that provider role exists

        let idkey = args.get_one::<String>("idkey").unwrap().to_string();
        match context.client.add_contact(idkey.clone()).await {
            Ok(_) => {
                // TODO share busy object

                Ok(Some(String::from(format!(
                    "Contact with idkey <{}> added",
                    idkey
                ))))
            }
            Err(err) => Ok(Some(String::from(format!(
                "Could not add contact: {}",
                err.to_string()
            )))),
        }
    }

    /*
    // Called by provider
    pub async fn add_client(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        // check that provider role exists
        let roles_id = ROLES_PREFIX.to_owned();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let roles_opt = data_store_guard.get_data(&roles_id);

        match roles_opt {
            Some(roles_obj) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();

                match roles.provider {
                    Some(provider) => {
                        // add client
                        let client_idkey =
                            args.get_one::<String>("client_idkey").unwrap().to_string();

                        match context.client.add_contact(client_idkey.clone()).await {
                            Ok(_) => Ok(Some(String::from(format!(
                                "Client with idkey <{}> added",
                                idkey
                            )))),
                            Err(err) => Ok(Some(String::from(format!(
                                "Could not add client: {}",
                                err.to_string()
                            )))),
                        }
                    }
                    None => Ok(Some(String::from(
                        "Provider role does not exist; cannot add client.",
                    ))),
                }
            }
            None => {
                Ok(Some(String::from("Roles do not exist; cannot add client")))
            }
        }
    }
    */

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

    pub fn get_perm(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("id").unwrap();
        let device_guard = context.client.device.read();
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let perm = meta_store_guard.get_perm(&id);

        Ok(Some(String::from(format!("{:?}", perm))))
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

    pub fn get_group(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("id").unwrap();
        let device_guard = context.client.device.read();
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let group = meta_store_guard.get_group(&id);

        Ok(Some(String::from(format!("{:?}", group))))
    }

    pub fn get_roles(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        // get roles
        let roles_id = ROLES_PREFIX.to_owned();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let roles_opt = data_store_guard.get_data(&roles_id);

        match roles_opt {
            Some(roles_obj) => {
                let roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();
                // TODO impl Display for Roles
                Ok(Some(String::from(format!("{:?}", roles))))
            }
            None => Ok(Some(String::from("No roles found."))),
        }
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
            Some(roles_obj) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();

                if init_provider {
                    //match args.get_many::<String>("durations") {
                    //    Some(arg_durations) => {
                    //        let durations = arg_durations
                    //            .map(|s| s.parse::<u32>().unwrap())
                    //            .collect::<Vec<u32>>();
                    //        roles.provider =
                    //            Some(Provider::new(Some(durations)));
                    //    }
                    //    None => roles.provider = Some(Provider::new(None)),
                    //}

                    roles.provider = Some(Provider::new(None));

                    // TODO init busy object
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

    /*
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
    */

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

        let pid = args.get_one::<String>("provider_id").unwrap().to_string();

        Ok(Some(String::from("TBD")))
    }

    // Called by client
    pub async fn request_appointment(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        // TODO check provider_id exists
        let provider_id = args.get_one::<String>("provider_id").unwrap();

        let notes = args.get_one::<String>("notes");

        // parse date
        let date_str = args.get_one::<String>("date").unwrap();
        match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            Ok(date) => {
                // parse time
                let time_str = args.get_one::<String>("time").unwrap();
                match NaiveTime::parse_from_str(time_str, "%H:%M:%S") {
                    Ok(time) => {
                        let appt =
                            AppointmentInfo::new(date, time, notes.cloned());
                        let mut id: String = APPT_PREFIX.to_owned();
                        id.push_str("/");
                        // TODO make method => create_new_prefixed_id()
                        id.push_str(&Uuid::new_v4().to_string());
                        let json_string = serde_json::to_string(&appt).unwrap();

                        // store appointment request
                        match context
                            .client
                            .set_data(
                                id.clone(),
                                APPT_PREFIX.to_owned(),
                                json_string,
                                None,
                            )
                            .await
                        {
                            Ok(_) => {
                                // share appointment request with provider
                                let vec = vec![provider_id];

                                // temporary hack b/c cannot set and share data
                                // at the same time, and sharing expects that
                                // the
                                // data already exists, so must wait for
                                // set_data
                                // message to return from the server
                                std::thread::sleep(
                                    std::time::Duration::from_secs(1),
                                );

                                match context
                                    .client
                                    .add_writers(id.clone(), vec.clone())
                                    .await {
                                    Ok(_) => Ok(Some(String::from(format!(
                                        "Successfully requested appointment with id {}",
                                        id.clone()
                                    )))),
                                    Err(err) => Ok(Some(String::from(format!(
                                        "Could not share appointment: {}",
                                        err.to_string()
                                    )))),
                                }
                            }
                            Err(err) => Ok(Some(String::from(format!(
                                "Could not store appointment: {}",
                                err.to_string()
                            )))),
                        }
                    }
                    Err(err) => Ok(Some(String::from(format!(
                        "Error parsing time: {}",
                        err
                    )))),
                }
            }
            Err(err) => {
                Ok(Some(String::from(format!("Error parsing date: {}", err))))
            }
        }
    }

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

        // TODO modify appointment
        // TODO does edit delete the previous appointment or not?

        Ok(Some(String::from("TBD")))
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
        //.with_command(Command::new("get_contacts"), CalendarApp::get_contacts)
        .with_command_async(
            Command::new("add_contact")
                .arg(Arg::new("idkey").required(true).short('i')),
            |args, context| Box::pin(CalendarApp::add_contact(args, context)),
        )
        //.with_command_async(
        //    Command::new("add_client").arg(Arg::new("client_idkey").
        // required(true).short('i')),    |args, context|
        // Box::pin(CalendarApp::add_client(args, context)),
        //)
        //.with_command_async(
        //    Command::new("add_provider").arg(Arg::new("provider_idkey").
        // required(true).short('p')),    |args, context|
        // Box::pin(CalendarApp::add_provider(args, context)),
        //)
        .with_command(
            Command::new("get_linked_devices"),
            CalendarApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), CalendarApp::get_data)
        .with_command(Command::new("get_perms"), CalendarApp::get_perms)
        .with_command(
            Command::new("get_perm")
                .arg(Arg::new("id").required(true).short('i').long("id")),
            CalendarApp::get_perm,
        )
        .with_command(Command::new("get_groups"), CalendarApp::get_groups)
        .with_command(
            Command::new("get_group")
                .arg(Arg::new("id").required(true).short('i').long("id")),
            CalendarApp::get_group,
        )
        .with_command(Command::new("get_roles"), CalendarApp::get_roles)
        //.with_command(Command::new("get_clients"), CalendarApp::get_clients)
        //.with_command(Command::new("get_providers"), CalendarApp::get_providers)
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
                //.arg(
                //    Arg::new("durations")
                //        .required(false)
                //        .action(ArgAction::Append)
                //        .long("durations")
                //        .short('d')
                //        .help("Set valid appointment duration options
                // (provider only)"),
                //)
                //.arg(
                //    Arg::new("blocked")
                //        .required(false)
                //        .action(ArgAction::Append)
                //        .long("blocked")
                //        .short('b')
                //        .help("Set blocked-off times (provider only)"),
                //)
                .arg(
                    Arg::new("client")
                        .required(false)
                        .action(ArgAction::SetTrue)
                        .long("client")
                        .short('c')
                        .help("Init client role"),
                ),
            |args, context| Box::pin(CalendarApp::init_role(args, context)),
        )
        .with_command(
            Command::new("get_appointment").arg(Arg::new("id").required(true)),
            CalendarApp::get_appointment,
        )
        //.with_command(
        //    Command::new("get_provider_appointments").arg(
        //        Arg::new("provider_id")
        //            .required(true)
        //            .long("provider_id")
        //            .short('p'),
        //    ),
        //    CalendarApp::get_provider_appointments,
        //)
        //.with_command(
        //    Command::new("get_client_appointments").arg(
        //        Arg::new("client_id")
        //            .required(true)
        //            .long("client_id")
        //            .short('c'),
        //    ),
        //    CalendarApp::get_client_appointments,
        //)
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
            Command::new("request_appointment")
                .arg(
                    Arg::new("provider_id")
                        .required(true)
                        .long("provider_id")
                        .short('p'),
                )
                .arg(
                    Arg::new("date")
                        .required(true)
                        .long("date")
                        .short('d')
                        .help("Format: YYYY-MM-DD"),
                )
                .arg(
                    Arg::new("time")
                        .required(true)
                        .long("time")
                        .short('t')
                        .help("Format: HH:MM:SS"),
                )
                .arg(
                    Arg::new("notes").required(false).long("notes").short('n'),
                ),
            |args, context| {
                Box::pin(CalendarApp::request_appointment(args, context))
            },
        )
        .with_command_async(
            Command::new("edit_appointment")
                .arg(Arg::new("id").required(true).long("id").short('i')),
            |args, context| {
                Box::pin(CalendarApp::edit_appointment(args, context))
            },
        );

    repl.run_async().await
}
