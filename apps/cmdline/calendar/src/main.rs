use chrono::naive::{NaiveDate, NaiveDateTime, NaiveTime};
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tank::client::TankClient;
use tank::data::ScubaData;
use uuid::Uuid;

/*
 * Calendar
 * - [x] allows patients to book appointments with providers given the provider's
 *   availability
 * - how appointments are made
 *   - [x] patient requests single time with provider, which the provider must then
 *     confirm + update their own availability
 *   - [ ] patient requests a prioritized list of appointment times which the provider
 *     can automatically confirm/deny based on the highest-pri slot that is available
 *   - [ ] provider executes the appointment confirmation and availability update in a
 *     transaction (serializability)
 * - [x] providers share their availability with all patients
 * - [x] appointments are private to provider and the patient whom the appointment is
 *   with, but update the provider's overall availability, visible to their other
 *   patients
 * - [ ] providers can also block off times on their end without needing an appointment
 *   to be scheduled, e.g. for lunch breaks
 * - [x] the same device can act as both a patient and provider
 * - [x] patients can have multiple providers
 * - [x] providers can have their own providers
 */

// FIXME impl more helper methods, a lot of repetitive code

// TODO use the struct name as the type/prefix instead
// https://users.rust-lang.org/t/how-can-i-convert-a-struct-name-to-a-string/66724/8
// or
// #[serde(skip_serializing_if = "path")] on all fields (still cumbersome),
// calling simple function w bool if only want struct name
// TODO remove roles and just have patient/provider objects
const ROLES_PREFIX: &str = "roles";
const APPT_PREFIX: &str = "appointment";
const AVAIL_PREFIX: &str = "availability";

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
 * is a patient, provider, or both.
 */

// TODO impl Display
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Provider {
    appointment_ids: Vec<String>,
    availability_id: String,
    //patients: Vec<String>,
}

impl Provider {
    fn new(_durations: Option<Vec<u32>>, availability_id: String) -> Self {
        Provider {
            appointment_ids: Vec::<String>::new(),
            availability_id,
            //patients: Vec::<String>::new(),
        }
    }
}

// TODO impl Display
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Patient {
    appointment_ids: Vec<String>,
    //providers: Vec<String>,
}

impl Patient {
    fn new() -> Self {
        Patient {
            appointment_ids: Vec::<String>::new(),
            //providers: Vec::<String>::new(),
        }
    }
}

// TODO impl Display
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Roles {
    provider: Option<Provider>,
    patient: Option<Patient>,
}

impl Roles {
    fn new() -> Self {
        Roles {
            provider: None,
            patient: None,
        }
    }
}

/*
 * The AppointmentInfo struct describes each appointment made and is
 * shared between patient and provider
 */

// TODO impl Display
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppointmentInfo {
    date: NaiveDate,
    time: NaiveTime,
    duration_min: u32,
    patient_notes: Option<String>,
    pending: bool,
    // TODO add perm field or resolved writer idkeys if patients make apptmts,
    // although data_store doesn't have access to metadata_store, so this would
    // constitute a larger change in Tank (the two are separate for locking
    // purposes now)
}

impl AppointmentInfo {
    fn new(
        date: NaiveDate,
        time: NaiveTime,
        patient_notes: Option<String>,
    ) -> AppointmentInfo {
        AppointmentInfo {
            date,
            time,
            duration_min: DEFAULT_DUR,
            patient_notes,
            pending: true,
        }
    }
}

/*
 * The Availability struct info is shared by providers with their
 * patients. It obfuscates all appointment details or blocked slots and
 * simply shows them all as "busy".
 */

// TODO impl Display
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Availability {
    busy_slots: HashMap<NaiveDateTime, u32>,
}

impl Availability {
    fn new() -> Self {
        Availability {
            busy_slots: HashMap::new(),
        }
    }

    fn add_busy_slot(&mut self, datetime: NaiveDateTime, duration: u32) -> Option<u32> {
        self.busy_slots.insert(datetime, duration)
    }
}

/*
 * Application logic below.
 */

#[derive(Clone)]
struct CalendarApp {
    client: TankClient,
}

impl CalendarApp {
    pub async fn new() -> CalendarApp {
        let client = TankClient::new(
            None, None, false, None, None,
            false, false, true, true, // serializability
            None, None, None, None, None, None, None, None, None, // benchmarking args
        )
        .await;
        Self { client }
    }

    fn exists_device(&self) -> bool {
        match self.client.device.read().as_ref() {
            Some(_) => true,
            None => false,
        }
    }

    fn new_prefixed_id(prefix: &String) -> String {
        let mut id: String = prefix.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        id
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

    pub async fn init_new_device(context: &mut Arc<Self>) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        context.client.create_standalone_device().await;

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
                None,
                false,
            )
            .await
        {
            Ok(_) => {
                context.client.end_transaction().await;
                Ok(Some(String::from("Standalone device created.")))
            }
            Err(err) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Could not create device: {}",
                    err.to_string()
                ))))
            }
        }
    }

    pub async fn init_linked_device(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        match context
            .client
            .create_linked_device(args.get_one::<String>("idkey").unwrap().to_string())
            .await
        {
            Ok(_) => {
                context.client.end_transaction().await;
                Ok(Some(String::from("Linked device created!")))
            }
            Err(err) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Could not create linked device: {}",
                    err.to_string()
                ))))
            }
        }
    }

    pub async fn get_name(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        context.client.end_transaction().await;
        Ok(Some(String::from(format!(
            "Name: {}",
            context.client.linked_name()
        ))))
    }

    pub async fn get_idkey(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        context.client.end_transaction().await;
        Ok(Some(String::from(format!(
            "Idkey: {}",
            context.client.idkey()
        ))))
    }

    pub async fn get_linked_devices(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let linked_devices = context.client.get_linked_devices().await.unwrap();
        context.client.end_transaction().await;
        Ok(Some(itertools::join(linked_devices, "\n")))
    }

    // Called by provider only; upon contact addition, provider shares
    // availability object with patient
    pub async fn add_patient(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        // TODO check that provider role exists (see below commented-out code)

        let idkey = args.get_one::<String>("idkey").unwrap().to_string();
        match context.client.add_contact(idkey.clone()).await {
            Ok(_) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Patient with idkey <{}> added",
                    idkey
                ))))
            }
            Err(err) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Could not add patient: {}",
                    err.to_string()
                ))))
            }
        }
    }

    // Called by provider
    pub async fn share_availability(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let patient = args.get_one::<String>("patient_name").unwrap().to_string();
        let vec = vec![&patient];
        match context.client.get_data(&ROLES_PREFIX.to_owned()).await {
            Ok(Some(roles_obj)) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();

                if let Some(provider_role) = roles.provider {
                    match context
                        .client
                        .add_do_readers(provider_role.availability_id, vec)
                        .await
                    {
                        Ok(_) => {
                            context.client.end_transaction().await;
                            Ok(Some(String::from(format!(
                                "Availability shared with patient {}",
                                patient
                            ))))
                        }
                        Err(err) => {
                            context.client.end_transaction().await;
                            Ok(Some(String::from(format!(
                                "Could not share availability: {}",
                                err.to_string()
                            ))))
                        }
                    }
                } else {
                    context.client.end_transaction().await;
                    return Ok(Some(String::from("No provider role initialized.")));
                }
            }
            Ok(None) => {
                context.client.end_transaction().await;
                Ok(Some(String::from("Roles do not exist.")))
            }
            Err(err) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Error getting roles: {}",
                    err.to_string()
                ))))
            }
        }
    }

    pub async fn get_data(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        if let Some(id) = args.get_one::<String>("id") {
            match context.client.get_data(id).await {
                Ok(Some(data)) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!("{}", data))))
                }
                Ok(None) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Data with id {} does not exist",
                        id
                    ))))
                }
                Err(err) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Could not get data: {}",
                        err.to_string()
                    ))))
                }
            }
        } else {
            let data = context.client.get_all_data().await.unwrap();
            context.client.end_transaction().await;
            Ok(Some(itertools::join(data, "\n")))
        }
    }

    pub async fn get_perms(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        if let Some(id) = args.get_one::<String>("id") {
            match context.client.get_perm(id).await {
                Ok(Some(perm)) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!("{}", perm))))
                }
                Ok(None) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Perm with id {} does not exist",
                        id
                    ))))
                }
                Err(err) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Could not get perm: {}",
                        err.to_string()
                    ))))
                }
            }
        } else {
            let perms = context.client.get_all_perms().await.unwrap();
            context.client.end_transaction().await;
            Ok(Some(itertools::join(perms, "\n")))
        }
    }

    pub async fn get_groups(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        if let Some(id) = args.get_one::<String>("id") {
            match context.client.get_group(id).await {
                Ok(Some(group)) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!("{}", group))))
                }
                Ok(None) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Group with id {} does not exist",
                        id
                    ))))
                }
                Err(err) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Could not get group: {}",
                        err.to_string()
                    ))))
                }
            }
        } else {
            let groups = context.client.get_all_groups().await.unwrap();
            context.client.end_transaction().await;
            Ok(Some(itertools::join(groups, "\n")))
        }
    }

    pub async fn get_roles(context: &mut Arc<Self>) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        match context.client.get_data(&ROLES_PREFIX.to_owned()).await {
            Ok(Some(roles_obj)) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();
                context.client.end_transaction().await;
                Ok(Some(String::from(format!("{:?}", roles))))
            }
            Ok(None) => {
                context.client.end_transaction().await;
                Ok(Some(String::from("Roles do not exist.")))
            }
            Err(err) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Error getting roles: {}",
                    err.to_string()
                ))))
            }
        }
    }

    pub async fn init_provider_role(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        match context.client.get_data(&ROLES_PREFIX.to_owned()).await {
            Ok(Some(roles_obj)) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();

                // init Availability object
                let avail = Availability::new();
                let mut avail_id: String = AVAIL_PREFIX.to_owned();
                avail_id.push_str("/");
                // <avail_id> = avail/<provider_id>
                avail_id.push_str(&context.client.linked_name());
                let json_avail = serde_json::to_string(&avail).unwrap();

                let res = context
                    .client
                    .set_data(
                        avail_id.clone(),
                        AVAIL_PREFIX.to_string(),
                        json_avail,
                        None,
                        None,
                        false,
                    )
                    .await;
                if res.is_err() {
                    context.client.end_transaction().await;
                    return Ok(Some(String::from(format!(
                        "Error creating provider availability: {}",
                        res.err().unwrap().to_string()
                    ))));
                }

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

                roles.provider = Some(Provider::new(None, avail_id.clone()));
                let json_roles = serde_json::to_string(&roles).unwrap();

                match context
                    .client
                    .set_data(
                        ROLES_PREFIX.to_string(),
                        ROLES_PREFIX.to_string(),
                        json_roles,
                        None,
                        None,
                        false,
                    )
                    .await
                {
                    Ok(_) => {
                        context.client.end_transaction().await;
                        Ok(Some(String::from("Created provider role.")))
                    }
                    Err(err) => {
                        context.client.end_transaction().await;
                        Ok(Some(String::from(format!(
                            "Error creating provider role: {}",
                            err.to_string()
                        ))))
                    }
                }
            }
            Ok(None) => {
                context.client.end_transaction().await;
                Ok(Some(String::from("Roles do not exist.")))
            }
            Err(err) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Error getting roles: {}",
                    err.to_string()
                ))))
            }
        }
    }

    pub async fn init_patient_role(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        if !context.exists_device() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        match context.client.get_data(&ROLES_PREFIX.to_owned()).await {
            Ok(Some(roles_obj)) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();

                roles.patient = Some(Patient::new());
                let json_roles = serde_json::to_string(&roles).unwrap();

                match context
                    .client
                    .set_data(
                        ROLES_PREFIX.to_string(),
                        ROLES_PREFIX.to_string(),
                        json_roles,
                        None,
                        None,
                        false,
                    )
                    .await
                {
                    Ok(_) => {
                        context.client.end_transaction().await;
                        Ok(Some(String::from("Created patient role.")))
                    }
                    Err(err) => {
                        context.client.end_transaction().await;
                        Ok(Some(String::from(format!(
                            "Error creating patient role: {}",
                            err.to_string()
                        ))))
                    }
                }
            }
            Ok(None) => {
                context.client.end_transaction().await;
                Ok(Some(String::from("Roles do not exist.")))
            }
            Err(err) => {
                context.client.end_transaction().await;
                Ok(Some(String::from(format!(
                    "Error getting roles: {}",
                    err.to_string()
                ))))
            }
        }
    }

    /*
    pub async fn get_availability_id(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        match context.client.get_data(&ROLES_PREFIX.to_owned()).await {
            Ok(Some(roles_obj)) => {
                let mut roles: Roles =
                    serde_json::from_str(roles_obj.data_val()).unwrap();

                if let Some(provider_role) = roles.provider {
                    return Ok(Some(String::from(format!(
                        "Availability id is {}",
                        provider_role.availability_id
                    ))));
                } else {
                    return Ok(Some(String::from("No provider role initialized.")));
                }
            }
            Ok(None) => Ok(Some(String::from("Roles do not exist."))),
            Err(err) => Ok(Some(String::from(format!(
                "Error getting roles: {}",
                err.to_string()
            )))),
        }
    }
    */

    // Called by either patient or provider
    /*
    pub async fn get_appointment(
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
                let val: AppointmentInfo =
                    serde_json::from_str(val_obj.data_val()).unwrap();
                Ok(Some(String::from(format!("{:?}", val))))
            }
            Ok(None) => Ok(Some(String::from(format!(
                "Appointment with id {} does not exist.",
                id,
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Error getting appointment: {}",
                err.to_string()
            )))),
        }
    }
    */

    /*
    // TODO
    // Called by patient (see all appointments with one provider)
    pub fn get_provider_appointments(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from("TBD")))
    }

    // TODO
    // Called by provider (see all appointments with one patient)
    pub fn get_patient_appointments(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        Ok(Some(String::from("TBD")))
    }
    */

    // TODO
    // Called by patient
    //pub fn view_provider_availability(
    //    args: ArgMatches,
    //    context: &mut Arc<Self>,
    //) -> ReplResult<Option<String>> {
    //    if !context.exists_device() {
    //        return Ok(Some(String::from(
    //            "Device does not exist, cannot run command.",
    //        )));
    //    }

    //    let pid = args.get_one::<String>("provider_id").unwrap().to_string();

    //    Ok(Some(String::from("TBD")))
    //}

    // Called by patient
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
        let date_res = NaiveDate::parse_from_str(date_str, "%Y-%m-%d");
        if date_res.is_err() {
            return Ok(Some(String::from(format!(
                "Error parsing date: {}",
                date_res.err().unwrap().to_string()
            ))));
        }

        // parse time
        let time_str = args.get_one::<String>("time").unwrap();
        let time_res = NaiveTime::parse_from_str(time_str, "%H:%M:%S");
        if time_res.is_err() {
            return Ok(Some(String::from(format!(
                "Error parsing time: {}",
                time_res.err().unwrap().to_string()
            ))));
        }

        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start first transaction.")));
        }

        let appt =
            AppointmentInfo::new(date_res.unwrap(), time_res.unwrap(), notes.cloned());
        let id = Self::new_prefixed_id(&APPT_PREFIX.to_string());
        let json_string = serde_json::to_string(&appt).unwrap();

        // store appointment request
        res = context
            .client
            .set_data(
                id.clone(),
                APPT_PREFIX.to_owned(),
                json_string,
                None,
                None,
                false,
            )
            .await;
        if res.is_err() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Could not store appointment: {}",
                res.err().unwrap().to_string()
            ))));
        }

        context.client.end_transaction().await;

        // temporary hack b/c cannot set and share data
        // at the same time, and sharing expects that the
        // data already exists, so must wait for set_data
        // message to return from the server
        std::thread::sleep(std::time::Duration::from_secs(1));

        res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start second transaction.")));
        }

        // share appointment request with provider
        let vec = vec![provider_id];

        res = context.client.add_writers(id.clone(), vec.clone()).await;
        if res.is_err() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Could not share appointment: {}",
                res.err().unwrap().to_string()
            ))));
        }

        context.client.end_transaction().await;

        Ok(Some(String::from(format!(
            "Successfully requested appointment with id {}",
            id.clone()
        ))))
    }

    // Called by provider
    pub async fn confirm_appointment(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let id = args.get_one::<String>("id").unwrap().to_string();

        let mut empty_res = context.client.start_transaction();
        if empty_res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        let mut res = context.client.get_data(&id).await;
        if res.is_err() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Error getting appointment: {}",
                res.err().unwrap().to_string()
            ))));
        } else if res.as_ref().unwrap().is_none() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Appointment with id {} does not exist.",
                id,
            ))));
        }

        // update pending field on appointment
        let mut appt: AppointmentInfo =
            serde_json::from_str(res.as_ref().unwrap().as_ref().unwrap().data_val())
                .unwrap();

        // TODO check that appointment doesn't conflict with any existing busy slots

        appt.pending = false;
        let json_string = serde_json::to_string(&appt).unwrap();

        // confirm appointment
        empty_res = context
            .client
            .set_data(
                id.clone(),
                APPT_PREFIX.to_owned(),
                json_string,
                None,
                None,
                false,
            )
            .await;
        if empty_res.is_err() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Could not update appointment: {}",
                res.err().unwrap().to_string()
            ))));
        }

        // update availability
        let datetime = NaiveDateTime::new(appt.date, appt.time);

        res = context.client.get_data(&ROLES_PREFIX.to_owned()).await;
        if res.is_err() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Error getting roles: {}",
                res.err().unwrap().to_string()
            ))));
        } else if res.as_ref().unwrap().is_none() {
            context.client.end_transaction().await;
            return Ok(Some(String::from("Roles do not exist.")));
        }

        // get availability id
        let mut roles: Roles =
            serde_json::from_str(res.unwrap().unwrap().data_val()).unwrap();

        if let Some(provider_role) = roles.provider {
            let mut res = context
                .client
                .get_data(&provider_role.availability_id)
                .await;
            if res.is_err() {
                context.client.end_transaction().await;
                return Ok(Some(String::from(format!(
                    "Error getting availability: {}",
                    res.err().unwrap().to_string()
                ))));
            }

            // add busy slot
            let mut avail: Availability =
                serde_json::from_str(res.unwrap().unwrap().data_val()).unwrap();
            avail.add_busy_slot(datetime, DEFAULT_DUR);
            let json_avail = serde_json::to_string(&avail).unwrap();

            // set new avail obj
            match context
                .client
                .set_data(
                    provider_role.availability_id,
                    AVAIL_PREFIX.to_string(),
                    json_avail,
                    None,
                    None,
                    false,
                )
                .await
            {
                Ok(_) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Confirmed appointment with id {}",
                        id
                    ))))
                }
                Err(err) => {
                    context.client.end_transaction().await;
                    Ok(Some(String::from(format!(
                        "Could not modify availability: {}",
                        err
                    ))))
                }
            }
        } else {
            context.client.end_transaction().await;
            return Ok(Some(String::from("No provider role initialized.")));
        }
    }

    // Called by patient
    //pub async fn edit_appointment(
    //    args: ArgMatches,
    //    context: &mut Arc<Self>,
    //) -> ReplResult<Option<String>> {
    //    if !context.exists_device() {
    //        return Ok(Some(String::from(
    //            "Device does not exist, cannot run command.",
    //        )));
    //    }

    //    // TODO modify appointment
    //    // TODO does edit delete the previous appointment or not?

    //    Ok(Some(String::from("TBD")))
    //}
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(CalendarApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Calendar App")
        .with_version("v0.1.0")
        .with_description("Scuba calendar app")
        .with_command_async(Command::new("init_new_device"), |_, context| {
            Box::pin(CalendarApp::init_new_device(context))
        })
        .with_command_async(
            Command::new("init_linked_device").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(CalendarApp::init_linked_device(args, context)),
        )
        .with_command(Command::new("check_device"), CalendarApp::check_device)
        .with_command_async(Command::new("get_name"), |_, context| Box::pin(CalendarApp::get_name(context)))
        .with_command_async(Command::new("get_idkey"), |_, context| Box::pin(CalendarApp::get_idkey(context)))
        .with_command_async(
            Command::new("add_patient").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(CalendarApp::add_patient(args, context)),
        )
        .with_command_async(
            Command::new("share_availability")
                .arg(Arg::new("patient_name").required(true)),
            |args, context| Box::pin(CalendarApp::share_availability(args, context)),
        )
        //.with_command_async(
        //    Command::new("add_patient").arg(Arg::new("patient_idkey").
        // required(true).short('i')),    |args, context|
        // Box::pin(CalendarApp::add_patient(args, context)),
        //)
        //.with_command_async(
        //    Command::new("add_provider").arg(Arg::new("provider_idkey").
        // required(true).short('p')),    |args, context|
        // Box::pin(CalendarApp::add_provider(args, context)),
        //)
        .with_command_async(Command::new("get_linked_devices"), |_, context| {
            Box::pin(CalendarApp::get_linked_devices(context))
        })
        .with_command_async(
            Command::new("get_data").arg(Arg::new("id").required(false)),
            |args, context| Box::pin(CalendarApp::get_data(args, context)),
        )
        .with_command_async(
            Command::new("get_perms").arg(Arg::new("id").required(false)),
            |args, context| Box::pin(CalendarApp::get_perms(args, context)),
        )
        .with_command_async(
            Command::new("get_groups").arg(Arg::new("id").required(false)),
            |args, context| Box::pin(CalendarApp::get_groups(args, context)),
        )
        .with_command_async(Command::new("get_roles"), |_, context| {
            Box::pin(CalendarApp::get_roles(context))
        })
        //.with_command(Command::new("get_patients"), CalendarApp::get_patients)
        //.with_command(Command::new("get_providers"), CalendarApp::get_providers)
        .with_command_async(Command::new("init_patient_role"), |_, context| {
            Box::pin(CalendarApp::init_patient_role(context))
        })
        .with_command_async(
            Command::new("init_provider_role"),
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
            |_, context| Box::pin(CalendarApp::init_provider_role(context)),
        )
        //.with_command_async(Command::new("get_availability_id"), |_, context| {
        //    Box::pin(CalendarApp::get_availability_id(context))
        //})
        //.with_command_async(
        //    Command::new("get_appointment").arg(Arg::new("id").required(true)),
        //    |args, context| Box::pin(CalendarApp::get_appointment(args, context)),
        //)
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
        //    Command::new("get_patient_appointments").arg(
        //        Arg::new("patient_id")
        //            .required(true)
        //            .long("patient_id")
        //            .short('c'),
        //    ),
        //    CalendarApp::get_patient_appointments,
        //)
        //.with_command(
        //    Command::new("view_provider_availability").arg(
        //        Arg::new("provider_id")
        //            .required(true)
        //            .long("provider_id")
        //            .short('p'),
        //    ),
        //    CalendarApp::view_provider_availability,
        //)
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
                        .help("Format: HH:MM:SS (24-hour)"),
                )
                .arg(Arg::new("notes").required(false).long("notes").short('n')),
            |args, context| Box::pin(CalendarApp::request_appointment(args, context)),
        )
        .with_command_async(
            Command::new("confirm_appointment").arg(Arg::new("id").required(true)),
            |args, context| Box::pin(CalendarApp::confirm_appointment(args, context)),
        );
    //.with_command_async(
    //    Command::new("edit_appointment")
    //        .arg(Arg::new("id").required(true).long("id").short('i')),
    //    |args, context| {
    //        Box::pin(CalendarApp::edit_appointment(args, context))
    //    },
    //);

    repl.run_async().await
}
