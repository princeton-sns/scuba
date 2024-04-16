use passwords::PasswordGenerator;
use serde::{Deserialize, Serialize};
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

const CONFIG_PREFIX: &str = "config";
const PASS_PREFIX: &str = "pass";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Password {
    pub config_id: String,
    username: String,
    password: String,
    otp_secret: String,
    otp_counter: u64,
}

impl Password {
    fn new(
        config_id: String,
        username: String,
        password: String,
        otp_secret: String,
    ) -> Self {
        Password {
            config_id,
            username,
            password,
            otp_secret,
            otp_counter: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    config_id: String,
    length: usize,
    numbers: bool,
    lowercase_letters: bool,
    uppercase_letters: bool,
    symbols: bool,
}

impl Config {
    fn new(
        config_id: String,
        length: usize,
        numbers: bool,
        lowercase_letters: bool,
        uppercase_letters: bool,
        symbols: bool,
    ) -> Self {
        Config {
            config_id,
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
    client: TankClient,
    ts: Arc<Mutex<Vec<(usize, String, Instant)>>>,
    run_ctr: Arc<RwLock<Option<usize>>>,
    app_filename: String,
}

impl PasswordManager {
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
    ) -> PasswordManager {
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

    pub async fn add_contact(&self, idkey: String) -> Result<(), Error> {
        self.client.add_contact(idkey).await
    }

    fn new_prefixed_id(prefix: &String) -> String {
        let mut id: String = prefix.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        id
    }

    pub async fn config_app_password(
        &self,
        app_name: String,
        length: usize,
        numbers: bool,
        lowercase: bool,
        uppercase: bool,
        symbols: bool,
    ) -> Result<String, Error> {
        let config = Config::new(
            app_name.clone(),
            length,
            numbers,
            lowercase,
            uppercase,
            symbols,
        );
        let mut id: String = CONFIG_PREFIX.to_owned();
        id.push_str("/");
        id.push_str(&app_name);
        let json_string = serde_json::to_string(&config).unwrap();
        let res = self
            .client
            .set_data(
                id.clone(),
                CONFIG_PREFIX.to_string(),
                json_string,
                None,
                None,
                false,
            )
            .await;
        if res.is_err() {
            return Err(res.err().unwrap());
        } else {
            return Ok(id);
        }
    }

    async fn gen_password_from_config(&self, config_id: &String) -> String {
        let config_opt = self.client.get_data(&config_id).await;
        let config = config_opt.unwrap().unwrap().clone();
        let config_obj: Config = serde_json::from_str(config.data_val()).unwrap();
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
        &self,
        config_id: &String,
        username: String,
        otp_secret: String,
        password: String,
    ) -> Result<String, Error> {
        //let password = self.gen_password_from_config(config_id).await;
        let pass_info =
            Password::new(config_id.to_string(), username, password, otp_secret);
        let id = Self::new_prefixed_id(&PASS_PREFIX.to_string());
        let json_string = serde_json::to_string(&pass_info).unwrap();
        let res = self
            .client
            .set_data(
                id.clone(),
                PASS_PREFIX.to_string(),
                json_string,
                None,
                None,
                false,
            )
            .await;
        if res.is_err() {
            return Err(res.err().unwrap());
        } else {
            return Ok(id);
        }
    }

    pub async fn update_password(
        &self,
        id: String,
        password: String,
    ) -> Result<(), Error> {
        let idx = self.run_ctr.read().await.unwrap();

        let mut password_obj: Password = serde_json::from_str(
            self.client.get_data(&id).await.unwrap().unwrap().data_val(),
        )
        .unwrap();

        self.ts
            .lock()
            .await
            .push((idx, String::from("enter update"), Instant::now()));

        //let new_pass =
        // self.gen_password_from_config(&password_obj.config_id).await;
        password_obj.password = password;
        let json_string = serde_json::to_string(&password_obj).unwrap();

        self.ts
            .lock()
            .await
            .push((idx, String::from("enter TANK"), Instant::now()));
        let res = self
            .client
            .set_data(
                id.clone(),
                PASS_PREFIX.to_string(),
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
            .push((idx, String::from("exit update"), Instant::now()));
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
            return Err(res.err().unwrap());
        } else {
            return Ok(());
        }
    }

    pub async fn add_readers(
        &self,
        pass_id: String,
        readers: Vec<&String>,
    ) -> Result<(), Error> {
        let res = self.client.add_readers(pass_id, readers).await;
        if res.is_err() {
            return Err(res.err().unwrap());
        } else {
            return Ok(());
        }
    }

    //fn hash(first: String, second: String) -> String {
    //    use base64ct::Encoding;
    //    use sha2::Digest;

    //    let mut hasher = sha2::Sha256::new();
    //    hasher.update(b"first");
    //    hasher.update(first);
    //    hasher.update(b"second");
    //    hasher.update(second);

    //    let hash = hasher.finalize();
    //    base64ct::Base64::encode_string(&hash)
    //}

    //pub async fn get_otp(&self, pass_id: String) -> Result<String, Error> {
    //    let res = context.client.get_data(&id).await;
    //    if res.is_err() { return Err(res.err().unwrap()); }
    //}
}

pub async fn run() {
    let num_warmup = 1000;
    let num_runs = 10000;
    let total_runs = num_runs + num_warmup;

    let num_core_send = 2 * total_runs;
    let num_core_recv = 2 * total_runs;
    let num_tank_send = total_runs;
    let num_tank_recv = 2 * total_runs;
    println!("num_core_send: {}", &num_core_send);
    println!("num_core_recv: {}", &num_core_recv);
    println!("num_tank_send: {}", &num_tank_send);
    println!("num_tank_recv: {}", &num_tank_recv);

    let base_dirname = "update_pass_data";
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

        let bw_out =
            format!("{}/{}c_{}r_bw_pmapp.txt", &dirname, &num_clients, &num_runs);
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
        let sender = PasswordManager::new(
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
            let client = PasswordManager::new(
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

        let sender_config = sender
            .config_app_password(String::from("netflix"), 14, true, true, true, true)
            .await
            .unwrap();
        // wait for write to propagate
        //std::thread::sleep(std::time::Duration::from_millis(100));

        for idkey in idkeys {
            if idkey == sender.client.idkey() {
                continue;
            }
            sender.add_contact(idkey).await;
        }

        let sender_pass = sender
            .add_password(
                &sender_config,
                String::from("nataliepopescu"),
                String::from("12345678901234567890"),
                String::from("j#LIO$#*UEYfs"),
            )
            .await
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        for (name, _) in clients.values() {
            sender.add_readers(sender_pass.clone(), vec![name]).await;
        }
        // wait for write to propagate
        //std::thread::sleep(std::time::Duration::from_millis(100));

        for run in 0..total_runs {
            sender
                .update_password(sender_pass.clone(), String::from("j8/#k$ddno2"))
                .await;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // avoid panicking because we couldn't receive the last message from the
    // server
    std::thread::sleep(std::time::Duration::from_millis(100));
}
