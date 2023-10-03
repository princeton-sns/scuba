use chrono::offset::Utc;
use chrono::DateTime;
use reedline_repl_rs::clap::{Arg, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use sequential_noise_kv::client::NoiseKVClient;
use sequential_noise_kv::data::NoiseData;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

/*
 * Family Social Media app
 * - [x] shares data across families
 * - [x] each user can belong to one or more families
 * - possible data:
 *   - [x] posts
 *   - [ ] comments
 *   - [ ] emoji reactions
 *   - [ ] chat groups
 *   - [x] live location
 *   - [ ] photos
 * - invariants:
 *   - [x] comment length
 *   - [ ] reaction type (subset of emojies)
 * - [ ] moderator permissions can be granted to users to help keep
 *   messages appropriate
 */

// TODO use the struct name as the type/prefix instead
// https://users.rust-lang.org/t/how-can-i-convert-a-struct-name-to-a-string/66724/8
// or
// #[serde(skip_serializing_if = "path")] on all fields (still cumbersome),
// calling simple function w bool if only want struct name
const FAM_PREFIX: &str = "family";
const POST_PREFIX: &str = "post";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Family {
    members: Vec<String>,
    // for easy time-based ordering
    posts: BTreeMap<DateTime<Utc>, String>,
}

impl Family {
    fn new(members: Vec<String>) -> Self {
        Family {
            members,
            posts: BTreeMap::new(),
        }
    }

    fn add_member(&mut self, id: &String) {
        self.members.push(id.to_string());
    }

    fn add_post(&mut self, post_time: DateTime<Utc>, post_id: String) {
        self.posts.insert(post_time, post_id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Post {
    family_id: String,
    contents: String,
    creation_time: DateTime<Utc>,
}

impl Post {
    fn new(family_id: &String, contents: &String) -> Self {
        Post {
            family_id: family_id.to_string(),
            contents: contents.to_string(),
            creation_time: Utc::now(),
        }
    }

    fn creation_time(&self) -> DateTime<Utc> {
        self.creation_time
    }
}

// TODO invariant val for post length

// TODO location sharing

#[derive(Clone)]
struct FamilyApp {
    client: NoiseKVClient,
}

impl FamilyApp {
    pub async fn new() -> FamilyApp {
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

    //pub fn get_contacts(
    //    _args: ArgMatches,
    //    context: &mut Arc<Self>,
    //) -> ReplResult<Option<String>> {
    //    if !context.exists_device() {
    //        return Ok(Some(String::from(
    //            "Device does not exist, cannot run command.",
    //        )));
    //    }

    //    Ok(Some(itertools::join(&context.client.get_contacts(), "\n")))
    //}

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

    pub async fn init_family(
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        let id = Self::new_prefixed_id(&FAM_PREFIX.to_string());
        let fam = Family::new(vec![context.client.linked_name()]);
        let json_fam = serde_json::to_string(&fam).unwrap();

        match context
            .client
            .set_data(id.clone(), FAM_PREFIX.to_string(), json_fam, None)
            .await
        {
            Ok(_) => {
                Ok(Some(String::from(format!("Family created with id {}", id))))
            }
            Err(err) => Ok(Some(String::from(format!(
                "Could not create family: {}",
                err.to_string()
            )))),
        }
    }

    pub async fn add_to_family(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let fam_id = args.get_one::<String>("fam_id").unwrap();
        let contact_name = args.get_one::<String>("contact_name").unwrap();

        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let fam_opt = data_store_guard.get_data(&fam_id);

        match fam_opt {
            Some(fam_obj) => {
                let mut fam: Family =
                    serde_json::from_str(fam_obj.data_val()).unwrap();
                fam.add_member(contact_name);
                let fam_json = serde_json::to_string(&fam).unwrap();
                core::mem::drop(data_store_guard);

                match context
                    .client
                    .set_data(
                        fam_id.clone(),
                        FAM_PREFIX.to_owned(),
                        fam_json,
                        None,
                    )
                    .await
                {
                    Ok(_) => {
                        // share family with new member
                        let sharees = vec![contact_name];

                        // temporary hack b/c cannot set and share data
                        // at the same time, and sharing expects that
                        // the
                        // data already exists, so must wait for
                        // set_data
                        // message to return from the server
                        std::thread::sleep(std::time::Duration::from_secs(1));

                        match context
                            .client
                            .add_writers(fam_id.clone(), sharees.clone())
                            .await
                        {
                            Ok(_) => Ok(Some(String::from(format!(
                                "Successfully shared family with id {}",
                                fam_id.clone()
                            )))),
                            Err(err) => Ok(Some(String::from(format!(
                                "Could not share family: {}",
                                err.to_string()
                            )))),
                        }
                    }
                    Err(err) => Ok(Some(String::from(format!(
                        "Could not store updated family: {}",
                        err.to_string()
                    )))),
                }
            }
            None => Ok(Some(String::from(format!(
                "Family with id {} does not exist.",
                fam_id,
            )))),
        }
    }

    pub async fn post_to_family(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let fam_id = args.get_one::<String>("fam_id").unwrap();
        let contents = args.get_one::<String>("post").unwrap();

        let device_guard = context.client.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let fam_opt = data_store_guard.get_data(&fam_id);

        // TODO might be good to put the three operations below in a transaction
        // (set post, share post, update family)

        match fam_opt {
            Some(fam_obj) => {
                // create post
                let post_id = Self::new_prefixed_id(&POST_PREFIX.to_string());
                let post = Post::new(fam_id, contents);
                let post_json = serde_json::to_string(&post).unwrap();

                match context
                    .client
                    .set_data(
                        post_id.clone(),
                        POST_PREFIX.to_owned(),
                        post_json,
                        None,
                    )
                    .await
                {
                    Ok(_) => {
                        // share post
                        let mut fam: Family =
                            serde_json::from_str(fam_obj.data_val()).unwrap();
                        let own_name = context.client.linked_name();
                        let sharees = fam
                            .members
                            .iter()
                            .filter(|&x| *x != own_name)
                            .collect::<Vec<&String>>();
                        core::mem::drop(data_store_guard);

                        // temporary hack b/c cannot set and share data
                        // at the same time, and sharing expects that
                        // the
                        // data already exists, so must wait for
                        // set_data
                        // message to return from the server
                        std::thread::sleep(std::time::Duration::from_secs(1));

                        match context
                            .client
                            .add_readers(post_id.clone(), sharees)
                            .await
                        {
                            Ok(_) => {
                                // add to family
                                fam.add_post(
                                    post.creation_time(),
                                    post_id.clone(),
                                );
                                let fam_json =
                                    serde_json::to_string(&fam).unwrap();

                                match context
                                    .client
                                    .set_data(
                                        fam_id.clone(),
                                        FAM_PREFIX.to_owned(),
                                        fam_json,
                                        None,
                                    )
                                    .await
                                {
                                    Ok(_) => Ok(Some(String::from(format!(
                                        "Post with id {} added to family with id {}",
                                        post_id, fam_id
                                    )))),
                                    Err(err) => Ok(Some(String::from(format!(
                                        "Could not store updated family: {}",
                                        err.to_string()
                                    )))),
                                }
                            }
                            Err(err) => Ok(Some(String::from(format!(
                                "Could not share post: {}",
                                err.to_string()
                            )))),
                        }
                    }
                    Err(err) => Ok(Some(String::from(format!(
                        "Could not store post: {}",
                        err.to_string()
                    )))),
                }
            }
            None => Ok(Some(String::from(format!(
                "Family with id {} does not exist.",
                fam_id,
            )))),
        }
    }

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
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(FamilyApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Family App")
        .with_version("v0.1.0")
        .with_description("Noise family app")
        .with_command(
            Command::new("init_new_device"),
            FamilyApp::init_new_device,
        )
        .with_command_async(
            Command::new("init_linked_device")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(FamilyApp::init_linked_device(args, context))
            },
        )
        .with_command(Command::new("check_device"), FamilyApp::check_device)
        .with_command(Command::new("get_name"), FamilyApp::get_name)
        .with_command(Command::new("get_idkey"), FamilyApp::get_idkey)
        //.with_command(Command::new("get_contacts"), FamilyApp::get_contacts)
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(FamilyApp::add_contact(args, context)),
        )
        .with_command(
            Command::new("get_linked_devices"),
            FamilyApp::get_linked_devices,
        )
        .with_command_async(Command::new("init_family"), |_, context| {
            Box::pin(FamilyApp::init_family(context))
        })
        .with_command_async(
            Command::new("add_to_family")
                .arg(Arg::new("fam_id").short('f').required(true))
                .arg(Arg::new("contact_name").short('c').required(true)),
            |args, context| Box::pin(FamilyApp::add_to_family(args, context)),
        )
        .with_command_async(
            Command::new("post_to_family")
                .arg(Arg::new("fam_id").short('f').required(true))
                .arg(Arg::new("post").short('p').required(true)),
            |args, context| Box::pin(FamilyApp::post_to_family(args, context)),
        )
        .with_command(
            Command::new("get_data").arg(Arg::new("id").required(false)),
            FamilyApp::get_data,
        )
        .with_command(Command::new("get_perms"), FamilyApp::get_perms)
        .with_command(
            Command::new("get_perm").arg(Arg::new("id").required(true)),
            FamilyApp::get_perm,
        )
        .with_command(Command::new("get_groups"), FamilyApp::get_groups)
        .with_command(
            Command::new("get_group").arg(Arg::new("id").required(true)),
            FamilyApp::get_group,
        );

    repl.run_async().await
}
