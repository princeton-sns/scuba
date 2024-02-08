//use passwords::PasswordGenerator;
use chrono::offset::Utc;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;
use tank::client::Error;
use tank::client::TankClient;
use tank::data::ScubaData;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

const MEMBER_PREFIX: &str = "member";
const FAM_PREFIX: &str = "family";
const POST_PREFIX: &str = "post";

const MAX_CHAR: usize = 400;

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
    comments: BTreeMap<DateTime<Utc>, String>,
}

impl Post {
    fn new(family_id: &String, contents: &String) -> Self {
        Post {
            family_id: family_id.to_string(),
            contents: contents.to_string(),
            creation_time: Utc::now(),
            comments: BTreeMap::new(),
        }
    }

    fn creation_time(&self) -> DateTime<Utc> {
        self.creation_time
    }

    fn add_comment(&mut self, comment_time: DateTime<Utc>, comment_id: String) {
        self.comments.insert(comment_time, comment_id);
    }
}

#[derive(Clone)]
struct FamilyApp {
    client: TankClient,
    ts: Arc<Mutex<Vec<(usize, String, Instant)>>>,
    run_ctr: Arc<RwLock<Option<usize>>>,
    app_filename: String,
}

impl FamilyApp {
    pub async fn new(
        num_core_send: Option<usize>,
        num_core_recv: Option<usize>,
        num_tank_send: Option<usize>,
        total_runs: Option<usize>,
        bw_filename: Option<String>,
        core_send_filename: Option<String>,
        core_recv_filename: Option<String>,
        tank_send_filename: Option<String>,
        tank_recv_filename_u: Option<String>,
        tank_recv_filename_d: Option<String>,
        app_filename: String,
    ) -> FamilyApp {
        let client = TankClient::new(
            None,
            None,
            false,
            None,
            None,
            // closed loop
            //false,
            //false,
            //true,
            // linearizability
            true,
            true,
            false,
            false,
            // benchmark args
            num_core_send,
            num_core_recv,
            num_tank_send,
            bw_filename,
            core_send_filename,
            core_recv_filename,
            tank_send_filename,
            tank_recv_filename_u,
            tank_recv_filename_d,
        )
        .await;
        client.create_standalone_device().await;

        Self {
            client,
            ts: Arc::new(Mutex::new(Vec::<(usize, String, Instant)>::new())),
            run_ctr: Arc::new(RwLock::new(total_runs)),
            app_filename,
        }
    }

    pub async fn init_device(&self) {
        // set post length invariant callback
        let device_guard = self.client.device.read();
        let mut data_store_guard = device_guard.as_ref().unwrap().data_store.write();
        data_store_guard.validator().set_validate_callback_for_type(
            POST_PREFIX.to_string(),
            |_, val| {
                let post: Post = serde_json::from_str(val.data_val()).unwrap();
                if post.contents.len() > MAX_CHAR {
                    return false;
                }
                true
            },
        );
    }

    pub async fn add_contact(&self, idkey: String) -> Result<(), Error> {
        self.client.add_contact(idkey).await
    }

    fn new_prefixed_id(prefix: &String) -> String {
        let mut id: String = prefix.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        id
    }

    pub async fn init_family(&self) -> Result<String, Error> {
        let id = Self::new_prefixed_id(&FAM_PREFIX.to_string());
        let fam = Family::new(vec![self.client.linked_name()]);
        let json_fam = serde_json::to_string(&fam).unwrap();
        let res = self
            .client
            .set_data(
                id.clone(),
                FAM_PREFIX.to_owned(),
                json_fam,
                None,
                None,
                false,
            )
            .await;
        if res.is_err() {
            return Err(res.err().unwrap());
        }
        Ok(id)
    }

    pub async fn add_to_family(
        &self,
        fam_id: String,
        contact_name: String,
    ) -> Result<(), Error> {
        let fam_obj = self.client.get_data(&fam_id).await.unwrap().unwrap();
        let mut fam: Family = serde_json::from_str(fam_obj.data_val()).unwrap();
        fam.add_member(&contact_name);
        let fam_json = serde_json::to_string(&fam).unwrap();
        let mut res = self
            .client
            .set_data(
                fam_id.clone(),
                FAM_PREFIX.to_owned(),
                fam_json,
                None,
                None,
                false,
            )
            .await;
        if res.is_err() {
            return Err(res.err().unwrap());
        }

        let sharees = vec![&contact_name];
        res = self.client.add_writers(fam_id, sharees).await;
        if res.is_err() {
            return Err(res.err().unwrap());
        }
        Ok(())
    }

    pub async fn edit_post(
        &self,
        post_id: String,
        new_contents: String,
    ) -> Result<(), Error> {
        let idx = self.run_ctr.read().await.unwrap();

        let mut post_obj: Post = serde_json::from_str(
            self.client
                .get_data(&post_id)
                .await
                .unwrap()
                .unwrap()
                .data_val(),
        )
        .unwrap();

        self.ts
            .lock()
            .await
            .push((idx, String::from("enter edit"), Instant::now()));

        post_obj.contents = new_contents;
        let json_string = serde_json::to_string(&post_obj).unwrap();

        self.ts
            .lock()
            .await
            .push((idx, String::from("enter TANK"), Instant::now()));
        let res = self
            .client
            .set_data(
                post_id.clone(),
                POST_PREFIX.to_owned(),
                json_string,
                None,
                None,
                true,
            )
            .await;
        self.ts
            .lock()
            .await
            .push((idx, String::from("exit TANK"), Instant::now()));

        self.ts
            .lock()
            .await
            .push((idx, String::from("exit edit"), Instant::now()));
        if idx == 1 {
            let mut f = File::options()
                .append(true)
                .create(true)
                .open(&self.app_filename)
                .unwrap();
            let vec = self.ts.lock().await;
            for entry in vec.iter() {
                write!(f, "{:?}\n", entry);
            }
        } else {
            *self.run_ctr.write().await = Some(idx - 1);
        }
        if res.is_err() {
            return res;
        }
        Ok(())
    }

    pub async fn post_to_family(
        &self,
        fam_id: String,
        contents: String,
    ) -> Result<String, Error> {
        let fam_obj = self.client.get_data(&fam_id).await.unwrap().unwrap();

        // create post
        let post_id = Self::new_prefixed_id(&POST_PREFIX.to_owned());
        let post = Post::new(&fam_id, &contents);
        let post_json = serde_json::to_string(&post).unwrap();

        let mut res = self
            .client
            .set_data(
                post_id.clone(),
                POST_PREFIX.to_owned(),
                post_json,
                None,
                None,
                false,
            )
            .await;
        if res.is_err() {
            return Err(res.err().unwrap());
        }

        // share post
        let mut fam: Family = serde_json::from_str(fam_obj.data_val()).unwrap();
        let self_name = self.client.linked_name();
        let sharees = fam
            .members
            .iter()
            .filter(|&x| *x != self_name)
            .collect::<Vec<&String>>();

        res = self.client.add_readers(post_id.clone(), sharees).await;
        if res.is_err() {
            return Err(res.err().unwrap());
        }

        // add post to family obj
        fam.add_post(post.creation_time(), post_id.clone());
        let fam_json = serde_json::to_string(&fam).unwrap();

        res = self
            .client
            .set_data(
                fam_id.clone(),
                FAM_PREFIX.to_owned(),
                fam_json,
                None,
                None,
                false,
            )
            .await;

        Ok(post_id)
    }
}

pub async fn run() {
    let num_warmup = 100;
    let num_runs = 1000;
    let total_runs = num_runs + num_warmup;

    let num_core_send = 2 * total_runs;
    let num_core_recv = 2 * total_runs;
    let num_tank_send = total_runs;
    let num_tank_recv = 2 * total_runs;
    println!("num_core_send: {}", &num_core_send);
    println!("num_core_recv: {}", &num_core_recv);
    println!("num_tank_send: {}", &num_tank_send);
    println!("num_tank_recv: {}", &num_tank_recv);

    let base_dirname = "edit_post_output";
    let mut idx = 0;
    let mut dirname: String;
    loop {
        dirname = String::from(format!("./{}_{}", &base_dirname, &idx));

        let res = fs::create_dir(dirname.clone());
        if res.is_ok() {
            break;
        }

        // if res.is_err(), dir already exists (empty or not), so let's
        // create a fresh one for this run
        idx += 1;
    }

    for num_clients in [1, 2, 4, 8, 16, 32] {
        println!("Running {} clients", &num_clients);

        let core_send_filename = format!(
            "{}/{}c_{}r_ts_core_send.txt",
            &dirname, &num_clients, &num_runs
        );
        let core_recv_filename = format!(
            "{}/{}c_{}r_ts_core_recv.txt",
            &dirname, &num_clients, &num_runs
        );
        let app_filename =
            format!("{}/{}c_{}r_ts_app.txt", &dirname, &num_clients, &num_runs);
        let tank_send_filename = format!(
            "{}/{}c_{}r_ts_tank_send.txt",
            &dirname, &num_clients, &num_runs
        );
        let tank_recv_filename_update = format!(
            "{}/{}c_{}r_ts_tank_recv_update.txt",
            &dirname, &num_clients, &num_runs
        );
        let tank_recv_filename_dummy = format!(
            "{}/{}c_{}r_ts_tank_recv_dummy.txt",
            &dirname, &num_clients, &num_runs
        );
        let tank_recv_filename_other = format!(
            "{}/{}c_{}r_ts_tank_recv_other.txt",
            &dirname, &num_clients, &num_runs
        );

        let bw_out = format!(
            "{}/{}c_{}r_bw_famapp.txt",
            &dirname, &num_clients, &num_runs
        );
        let mut f_bw = File::options()
            .append(true)
            .create(true)
            .open(bw_out.clone())
            .unwrap();
        write!(
            f_bw,
            "NEW EXP: {} runs, {} clients\n",
            &num_runs, &num_clients
        );

        let mut clients = HashMap::new();
        let sender = FamilyApp::new(
            Some(num_core_send),
            Some(num_core_recv),
            Some(total_runs),
            Some(total_runs),
            Some(bw_out),
            Some(core_send_filename.clone()),
            Some(core_recv_filename.clone()),
            Some(tank_send_filename.clone()),
            Some(tank_recv_filename_update.clone()),
            Some(tank_recv_filename_dummy.clone()),
            app_filename.clone(),
        )
        .await;

        let sender_idkey = sender.client.idkey();
        clients.insert(
            sender_idkey.clone(),
            (sender.client.linked_name(), sender.clone()),
        );

        for i in 1..num_clients {
            let client = FamilyApp::new(
                None,
                None,
                None,
                None,
                None,
                Some(core_send_filename.clone()),
                Some(core_recv_filename.clone()),
                Some(tank_send_filename.clone()),
                Some(tank_recv_filename_other.clone()),
                Some(tank_recv_filename_other.clone()),
                app_filename.clone(),
            )
            .await;
            clients.insert(client.client.idkey(), (client.client.linked_name(), client));
        }

        let idkeys: Vec<String> = clients.keys().cloned().collect();

        let fam_id = sender.init_family().await.unwrap();

        for idkey in idkeys {
            if idkey == sender.client.idkey() {
                continue;
            }
            sender.add_contact(idkey).await;
        }

        for (name, _) in clients.values() {
            sender.add_to_family(fam_id.clone(), name.clone()).await;
        }

        let first_post = "X".repeat(MAX_CHAR);
        let edited_post = "O".repeat(MAX_CHAR);

        let post_id = sender
            .post_to_family(fam_id.clone(), first_post)
            .await
            .unwrap();
        // wait for write to propagate
        std::thread::sleep(std::time::Duration::from_millis(50));

        for run in 0..total_runs {
            sender.edit_post(post_id.clone(), edited_post.clone()).await;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // avoid panicking because we couldn't receive the last message from the
    // server
    std::thread::sleep(std::time::Duration::from_millis(100));
}
