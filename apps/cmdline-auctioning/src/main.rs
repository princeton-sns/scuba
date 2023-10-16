use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use sequential_noise_kv::client::NoiseKVClient;
use sequential_noise_kv::data::NoiseData;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/*
 * Online Auctioning
 * - [ ] open auctions
 *   - [ ] announces start time
 *   - [ ] accepts auction connections
 * - [ ] closed auctions
 *   - [ ] auction starts between set clients
 * - [ ] auctioneer that
 *   - [ ] opens the auction
 *   - [ ] accepts and propagates bids
 *   - [ ] announces the final sale
 * - [ ] auctioneer that can be
 *   - [ ] the item seller
 *   - [ ] another trusted client
 */

// TODO use the struct name as the type/prefix instead
// https://users.rust-lang.org/t/how-can-i-convert-a-struct-name-to-a-string/66724/8
// or
// #[serde(skip_serializing_if = "path")] on all fields (still cumbersome),
// calling simple function w bool if only want struct name
const AUCTION_PREFIX: &str = "auction";
const BID_PREFIX: &str = "bid";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Auction {
    item_description: String,
    highest_bid: u64,
    highest_bidder: String,
}

impl Auction {
    fn new(
        item_description: String,
        highest_bid: u64,
        highest_bidder: String,
    ) -> Self {
        Auction {
            item_description,
            highest_bid,
            highest_bidder,
        }
    }

    fn update_highest_bid(
        &mut self,
        new_highest_bid: u64,
        new_highest_bidder: String,
    ) {
        self.highest_bid = new_highest_bid;
        self.highest_bidder = new_highest_bidder;
    }

    // returns true if updated, false otherwise
    fn submit_bid(&mut self, bid: &Bid) -> bool {
        if bid.bid > self.highest_bid {
            self.update_highest_bid(bid.bid, bid.bidder.clone());
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bid {
    auction_id: String,
    bid: u64,
    bidder: String,
}

impl Bid {
    fn new(auction_id: String, bid: u64, bidder: String) -> Self {
        Bid {
            auction_id,
            bid,
            bidder,
        }
    }
}

#[derive(Clone)]
struct AuctioningApp {
    client: NoiseKVClient,
}

impl AuctioningApp {
    pub async fn new() -> AuctioningApp {
        let client = NoiseKVClient::new(None, None, false, None, None).await;
        Self { client }
    }

    fn new_prefixed_id(prefix: &String) -> String {
        let mut id: String = prefix.to_owned();
        id.push_str("/");
        id.push_str(&Uuid::new_v4().to_string());
        id
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

    // called by auctioneer
    pub async fn create_auction(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let item_descr = args
            .get_one::<String>("item_description")
            .unwrap()
            .to_string();
        let starting_bid_str =
            args.get_one::<String>("starting_bid").unwrap().to_string();
        let starting_bid: u64 = starting_bid_str.parse().unwrap();
        let auction = Auction::new(
            item_descr,
            starting_bid,
            context.client.linked_name(),
        );
        let auction_id = Self::new_prefixed_id(&AUCTION_PREFIX.to_string());
        let auction_json = serde_json::to_string(&auction).unwrap();

        match context
            .client
            .set_data(auction_id, AUCTION_PREFIX.to_owned(), auction_json, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Successfully created auction"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create auction: {}",
                err.to_string()
            )))),
        }
    }

    // called by buyer
    pub async fn create_bid(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let auction_id =
            args.get_one::<String>("auction_id").unwrap().to_string();
        let bid_str = args.get_one::<String>("bid").unwrap().to_string();
        let bid_val: u64 = bid_str.parse().unwrap();
        let bid = Bid::new(auction_id, bid_val, context.client.linked_name());
        let bid_id = Self::new_prefixed_id(&BID_PREFIX.to_string());
        let bid_json = serde_json::to_string(&bid).unwrap();

        match context
            .client
            .set_data(bid_id, BID_PREFIX.to_owned(), bid_json, None)
            .await
        {
            Ok(_) => Ok(Some(String::from("Successfully created bid"))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create bid: {}",
                err.to_string()
            )))),
        }
    }

    // called by auctioneer
    pub async fn submit_bid(
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

        let bid_id = args.get_one::<String>("bid_id").unwrap().to_string();
        let bid_data = data_store_guard.get_data(&bid_id).unwrap();
        let bid: Bid = serde_json::from_str(bid_data.data_val()).unwrap();

        let auction_id = bid.auction_id.clone();
        let auction_data = data_store_guard.get_data(&auction_id).unwrap();
        let mut auction: Auction =
            serde_json::from_str(auction_data.data_val()).unwrap();

        if auction.submit_bid(&bid) {
            let auction_json = serde_json::to_string(&auction).unwrap();
            match context
                .client
                .set_data(
                    auction_id,
                    AUCTION_PREFIX.to_string(),
                    auction_json,
                    None,
                )
                .await
            {
                Ok(_) => Ok(Some(String::from("Bid submitted!"))),
                Err(err) => Ok(Some(String::from(format!(
                    "Could not submit bid: {}",
                    err.to_string()
                )))),
            }
        } else {
            Ok(Some(String::from("Bid too low")))
        }
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(AuctioningApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Auctioning App")
        .with_version("v0.1.0")
        .with_description("Noise online auctioning app")
        .with_command(
            Command::new("init_new_device"),
            AuctioningApp::init_new_device,
        )
        .with_command_async(
            Command::new("init_linked_device")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(AuctioningApp::init_linked_device(args, context))
            },
        )
        .with_command(Command::new("check_device"), AuctioningApp::check_device)
        .with_command(Command::new("get_name"), AuctioningApp::get_name)
        .with_command(Command::new("get_idkey"), AuctioningApp::get_idkey)
        //.with_command(
        //    Command::new("get_contacts").about("broken - don't use"),
        //    AuctioningApp::get_contacts,
        //)
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(AuctioningApp::add_contact(args, context)),
        )
        .with_command(
            Command::new("get_linked_devices"),
            AuctioningApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), AuctioningApp::get_data)
        .with_command(Command::new("get_perms"), AuctioningApp::get_perms)
        .with_command(Command::new("get_groups"), AuctioningApp::get_groups)
        .with_command_async(
            Command::new("create_auction")
                .arg(
                    Arg::new("item_description")
                        .required(true)
                        .long("item_description")
                        .short('d'),
                )
                .arg(
                    Arg::new("starting_bid")
                        .required(true)
                        .long("starting_bid")
                        .short('b'),
                ),
            |args, context| {
                Box::pin(AuctioningApp::create_auction(args, context))
            },
        )
        .with_command_async(
            Command::new("create_bid")
                .arg(
                    Arg::new("auction_id")
                        .required(true)
                        .long("auction_id")
                        .short('i'),
                )
                .arg(Arg::new("bid").required(true).long("bid").short('b')),
            |args, context| Box::pin(AuctioningApp::create_bid(args, context)),
        )
        .with_command_async(
            Command::new("submit_bid").arg(
                Arg::new("bid_id").required(true).long("bid_id").short('i'),
            ),
            |args, context| Box::pin(AuctioningApp::submit_bid(args, context)),
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
            |args, context| Box::pin(AuctioningApp::share(args, context)),
        );

    repl.run_async().await
}
