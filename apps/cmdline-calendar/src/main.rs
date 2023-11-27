use chrono::naive::{NaiveDate, NaiveDateTime, NaiveTime};
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use multi_key_dal::client::NoiseKVClient;
use multi_key_dal::data::NoiseData;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/*
 * Calendar
 * - [x] allows patients to book appointments with providers given the
 *   provider's availability
 * - how appointments are made
 *   - [x] patient requests single time with provider, which the provider
 *     must then confirm + update their own availability
 *   - [ ] patient requests a prioritized list of appointment times which
 *     the provider can automatically confirm/deny based on the
 *     highest-pri slot that is available
 *   - [~] provider puts the appointment confirmation and availability
 *     update in a transaction (serializability)
 * - [x] providers share their availability with all patients
 * - [x] appointments are private to provider and the patient whom the
 *   appointment is with, but update the provider's overall
 *   availability, visible to their other patients
 * - [ ] providers can also block off times on their end without needing
 *   an appointment to be scheduled, e.g. for lunch breaks
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
    //patients: Vec<String>,
}

impl Provider {
    fn new(durations: Option<Vec<u32>>) -> Self {
        Provider {
            appointment_ids: Vec::<String>::new(),
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
    // constitute a larger change in NoiseKV (the two are separate for locking
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

    fn add_busy_slot(
        &mut self,
        datetime: NaiveDateTime,
        duration: u32,
    ) -> Option<u32> {
        self.busy_slots.insert(datetime, duration)
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
        let client = NoiseKVClient::new(
            None,
            None,
            false,
            Some("calendar.txt"),
            None,
            None,
            // TODO fix for multi-key
            true,
            false,
            true,
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

    pub async fn init_new_device(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
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

    // Called by provider only; upon contact addition, provider shares
    // availability object with patient
    pub async fn add_patient(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        // TODO check that provider role exists (see below commented-out code)

        let idkey = args.get_one::<String>("idkey").unwrap().to_string();
        match context.client.add_contact(idkey.clone()).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Patient with idkey <{}> added",
                idkey
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not add patient: {}",
                err.to_string()
            )))),
        }
    }

    // Called by provider
    pub async fn share_availability(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let patient = args.get_one::<String>("patient_name").unwrap().to_string();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let vec = vec![&patient];

        let mut avail_id: String = AVAIL_PREFIX.to_owned();
        avail_id.push_str("/");
        // <avail_id> = avail/<provider_id>
        avail_id.push_str(&context.client.linked_name());

        match context.client.add_do_readers(avail_id, vec).await {
            Ok(_) => Ok(Some(String::from(format!(
                "Availability shared with patient {}",
                patient
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not share availability: {}",
                err.to_string()
            )))),
        }
    }

    /*
    // Called by provider
    pub async fn add_patient(
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
                        // add patient
                        let patient_idkey =
                            args.get_one::<String>("patient_idkey").unwrap().to_string();

                        match context.client.add_contact(patient_idkey.clone()).await {
                            Ok(_) => Ok(Some(String::from(format!(
                                "Patient with idkey <{}> added",
                                idkey
                            )))),
                            Err(err) => Ok(Some(String::from(format!(
                                "Could not add patient: {}",
                                err.to_string()
                            )))),
                        }
                    }
                    None => Ok(Some(String::from(
                        "Provider role does not exist; cannot add patient.",
                    ))),
                }
            }
            None => {
                Ok(Some(String::from("Roles do not exist; cannot add patient")))
            }
        }
    }
    */

    pub fn get_data(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        if let Some(id) = args.get_one::<String>("id") {
            match data_store_guard.get_data(id) {
                Some(data) => Ok(Some(String::from(format!("{}", data)))),
                None => Ok(Some(String::from(format!(
                    "Data with id {} does not exist",
                    id
                )))),
            }
        } else {
            let data = data_store_guard.get_all_data().values();
            Ok(Some(itertools::join(data, "\n")))
        }
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
        let perm_opt = meta_store_guard.get_perm(&id);

        match perm_opt {
            Some(perm) => Ok(Some(String::from(format!("{}", perm)))),
            None => Ok(Some(String::from(format!(
                "Perm with id {} does not exist",
                id
            )))),
        }
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
        let group_opt = meta_store_guard.get_group(&id);

        match group_opt {
            Some(group) => Ok(Some(String::from(format!("{}", group)))),
            None => Ok(Some(String::from(format!(
                "Group with id {} does not exist",
                id
            )))),
        }
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
        let mut init_patient = false;
        if args.get_flag("provider") {
            init_provider = true;
        }
        if args.get_flag("patient") {
            init_patient = true;
        }

        if !init_patient && !init_provider {
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
                            avail_id,
                            AVAIL_PREFIX.to_string(),
                            json_avail,
                            None,
                            None,
                        )
                        .await;
                    if res.is_err() {
                        return Ok(Some(String::from(format!(
                            "Error creating availability: {}",
                            res.err().unwrap().to_string()
                        ))));
                    }
                }

                if init_patient {
                    roles.patient = Some(Patient::new());
                }

                let json_string = serde_json::to_string(&roles).unwrap();

                match context
                    .client
                    .set_data(
                        roles_id.clone(),
                        ROLES_PREFIX.to_string(),
                        json_string,
                        None,
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

    // Called by either patient or provider
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
            Some(val_obj) => {
                let val: AppointmentInfo =
                    serde_json::from_str(val_obj.data_val()).unwrap();
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
    pub fn view_provider_availability(
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
        match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            Ok(date) => {
                // parse time
                let time_str = args.get_one::<String>("time").unwrap();
                match NaiveTime::parse_from_str(time_str, "%H:%M:%S") {
                    Ok(time) => {
                        let appt =
                            AppointmentInfo::new(date, time, notes.cloned());
                        let id =
                            Self::new_prefixed_id(&APPT_PREFIX.to_string());
                        let json_string = serde_json::to_string(&appt).unwrap();

                        // store appointment request
                        match context
                            .client
                            .set_data(
                                id.clone(),
                                APPT_PREFIX.to_owned(),
                                json_string,
                                None,
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

        // update pending field on appointment
        let id = args.get_one::<String>("appt_id").unwrap().to_string();
        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let appt_opt = data_store_guard.get_data(&id);

        if appt_opt.is_none() {
            return Ok(Some(String::from(format!(
                "Appointment with id {} does not exist.",
                id,
            ))));
        }

        let mut appt: AppointmentInfo =
            serde_json::from_str(appt_opt.unwrap().data_val()).unwrap();

        // TODO check that appointment doesn't conflict with any
        // existing busy slots

        appt.pending = false;
        let json_string = serde_json::to_string(&appt).unwrap();

        let mut res = context.client.start_transaction();
        if res.is_err() {
            return Ok(Some(String::from("Cannot start transaction.")));
        }

        res = context
            .client
            .set_data(
                id.clone(),
                APPT_PREFIX.to_owned(),
                json_string,
                None,
            )
            .await;
        if res.is_err() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Could not confirm appointment: {}",
                res.err().unwrap().to_string()
            ))));
        }

        // update availability
        let datetime = NaiveDateTime::new(appt.date, appt.time);
        let avail_opt = data_store_guard
            .get_data(&AVAIL_PREFIX.to_string());

        if avail_opt.is_none() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(
                "Availability object does not exist - bug.",
            )));
        }

        let mut avail: Availability =
            serde_json::from_str(avail_opt.unwrap().data_val())
                .unwrap();
        avail.add_busy_slot(datetime, DEFAULT_DUR);
        let json_avail =
            serde_json::to_string(&avail).unwrap();

        res = context
            .client
            .set_data(
                AVAIL_PREFIX.to_string(),
                AVAIL_PREFIX.to_string(),
                json_avail,
                None,
            )
            .await;
        if res.is_err() {
            context.client.end_transaction().await;
            return Ok(Some(String::from(format!(
                "Could not modify availability: {}",
                res.err().unwrap().to_string()
            ))));
        }

        context.client.end_transaction().await;
        Ok(Some(String::from(format!(
            "Confirmed appointment with id {}",
            id
        ))))
    }

    // Called by patient
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
        .with_command_async(
            Command::new("add_patient").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(CalendarApp::add_patient(args, context)),
        )
        .with_command_async(
            Command::new("share_availability")
                .arg(Arg::new("patient_name").required(true)),
            |args, context| {
                Box::pin(CalendarApp::share_availability(args, context))
            },
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
        .with_command_async(
            Command::new("get_linked_devices"),
            |_, context| {
                Box::pin(CalendarApp::get_linked_devices(context))
            },
        )
        .with_command(
            Command::new("get_data").arg(Arg::new("id").required(false)),
            CalendarApp::get_data,
        )
        .with_command(Command::new("get_perms"), CalendarApp::get_perms)
        .with_command(
            Command::new("get_perm").arg(Arg::new("id").required(true)),
            CalendarApp::get_perm,
        )
        .with_command(Command::new("get_groups"), CalendarApp::get_groups)
        .with_command(
            Command::new("get_group").arg(Arg::new("id").required(true)),
            CalendarApp::get_group,
        )
        .with_command(Command::new("get_roles"), CalendarApp::get_roles)
        //.with_command(Command::new("get_patients"), CalendarApp::get_patients)
        //.with_command(Command::new("get_providers"), CalendarApp::get_providers)
        .with_command_async(
            Command::new("init_role")
                .arg(
                    Arg::new("provider")
                        .required(false)
                        .action(ArgAction::SetTrue)
                        .long("provider")
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
                    Arg::new("patient")
                        .required(false)
                        .action(ArgAction::SetTrue)
                        .long("patient")
                        .help("Init patient role"),
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
            Command::new("confirm_appointment").arg(
                Arg::new("appt_id")
                    .required(true)
                    .long("appt_id")
                    .short('i'),
            ),
            |args, context| {
                Box::pin(CalendarApp::confirm_appointment(args, context))
            },
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
