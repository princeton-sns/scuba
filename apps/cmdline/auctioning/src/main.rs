use chrono::naive::{NaiveDate, NaiveTime};
use chrono::offset::Local;
use core::cmp::Ordering;
use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use single_key_tank::client::TankClient;
use single_key_tank::data::ScubaData;
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
const OPEN_CLIENTS_PREFIX: &str = "open_clients";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Auction {
    item_description: String,
    start_date: NaiveDate,
    start_time: NaiveTime,
    end_date: NaiveDate,
    end_time: NaiveTime,
    highest_bid: u64,
    highest_bidder: String,
    sold_price: Option<u64>,
    sold_bidder: Option<String>,
}

impl Auction {
    fn new(
        item_description: String,
        start_date: NaiveDate,
        start_time: NaiveTime,
        end_date: NaiveDate,
        end_time: NaiveTime,
        highest_bid: u64,
        highest_bidder: String,
    ) -> Self {
        Auction {
            item_description,
            start_date,
            start_time,
            end_date,
            end_time,
            //start_date_time: NaiveDateTime::new(start_date, start_time),
            //end_date_time: NaiveDateTime::new(end_date, end_time),
            highest_bid,
            highest_bidder,
            sold_price: None,
            sold_bidder: None,
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
    fn apply_bid(&mut self, bid: &Bid) -> bool {
        let cur_datetime = Local::now();
        let cur_date = cur_datetime.date_naive();
        let cur_time = cur_datetime.time();

        // check after start time
        if cur_date.cmp(&self.start_date) == Ordering::Less {
            println!("DATE TOO EARLY.");
            return false;
        }
        if cur_time.cmp(&self.start_time) == Ordering::Less {
            println!("TIME TOO EARLY.");
            return false;
        }

        // check before end time
        if cur_date.cmp(&self.end_date) == Ordering::Greater {
            println!("DATE TOO LATE.");
            return false;
        }
        if cur_time.cmp(&self.end_time) == Ordering::Greater {
            println!("TIME TOO LATE.");
            return false;
        }

        if bid.bid > self.highest_bid {
            self.update_highest_bid(bid.bid, bid.bidder.clone());
            return true;
        }
        false
    }

    fn is_over(&self) -> bool {
        let cur_datetime = Local::now();
        let cur_date = cur_datetime.date_naive();
        let cur_time = cur_datetime.time();

        // check after end time
        if cur_date.cmp(&self.end_date) == Ordering::Less {
            println!("DATE TOO EARLY.");
            return false;
        }
        if cur_time.cmp(&self.end_time) == Ordering::Less {
            println!("TIME TOO EARLY.");
            return false;
        }
        true
    }

    fn update_sale(&mut self) {
        self.sold_price = Some(self.highest_bid);
        self.sold_bidder = Some(self.highest_bidder.clone());
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

// takes place of contacts list
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAuctionClients {
    client_ids: Vec<String>,
}

impl OpenAuctionClients {
    fn new() -> Self {
        OpenAuctionClients {
            client_ids: Vec::new(),
        }
    }

    fn add_client(&mut self, client_id: &String) {
        self.client_ids.push(client_id.clone());
    }
}

#[derive(Clone)]
struct AuctioningApp {
    client: TankClient,
}

impl AuctioningApp {
    pub async fn new() -> AuctioningApp {
        let client = TankClient::new(
            None,
            None,
            false,
            None,
            None,
            // sequential consistency
            true,
            false,
            false,
            false,
            // benchmarking args
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
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
            "Successfully shared {}",
            id
        ))))
    }

    // called by auctioneer for announcing open auctions
    pub async fn add_to_open_auction_list(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let client_id =
            args.get_one::<String>("client_id").unwrap().to_string();
        let open_clients_opt = context
            .client
            .get_data(&OPEN_CLIENTS_PREFIX.to_string())
            .await
            .unwrap();
        if open_clients_opt.is_none() {
            let mut open_clients = OpenAuctionClients::new();
            open_clients.add_client(&client_id);
            let open_clients_json =
                serde_json::to_string(&open_clients).unwrap();

            match context
                .client
                .set_data(
                    OPEN_CLIENTS_PREFIX.to_string(),
                    OPEN_CLIENTS_PREFIX.to_string(),
                    open_clients_json,
                    None,
                    None,
                    false,
                )
                .await
            {
                Ok(_) => Ok(Some(String::from(
                    "Created and added client to open auction list",
                ))),
                Err(err) => Ok(Some(String::from(format!(
                    "Could not store open auction list: {}",
                    err.to_string()
                )))),
            }
        } else {
            let mut open_clients: OpenAuctionClients =
                serde_json::from_str(&open_clients_opt.unwrap().data_val())
                    .unwrap();
            open_clients.add_client(&client_id);
            let open_clients_json =
                serde_json::to_string(&open_clients).unwrap();

            match context
                .client
                .set_data(
                    OPEN_CLIENTS_PREFIX.to_string(),
                    OPEN_CLIENTS_PREFIX.to_string(),
                    open_clients_json,
                    None,
                    None,
                    false,
                )
                .await
            {
                Ok(_) => {
                    Ok(Some(String::from("Added client to open auction list")))
                }
                Err(err) => Ok(Some(String::from(format!(
                    "Could not store open auction list: {}",
                    err.to_string()
                )))),
            }
        }
    }

    // called by auctioneer for announcing open auctions
    pub async fn announce_open_auction(
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
        let open_clients_opt = context
            .client
            .get_data(&OPEN_CLIENTS_PREFIX.to_string())
            .await
            .unwrap();
        if open_clients_opt.is_none() {
            return Ok(Some(String::from(
                "Open clients list does not exist: add clients for open auction announcements."
            )));
        }
        let open_clients: OpenAuctionClients =
            serde_json::from_str(&open_clients_opt.unwrap().data_val())
                .unwrap();
        let clients = open_clients.client_ids.iter().collect::<Vec<&String>>();
        match context
            .client
            .add_readers(auction_id.clone(), clients)
            .await
        {
            Ok(_) => Ok(Some(String::from("Open auction announced."))),
            Err(err) => Ok(Some(String::from(format!(
                "Error announcing open auction: {}",
                err.to_string()
            )))),
        }
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

        let start_date = NaiveDate::parse_from_str(
            args.get_one::<String>("start_date").unwrap(),
            "%Y-%m-%d",
        )
        .unwrap();
        let start_time = NaiveTime::parse_from_str(
            args.get_one::<String>("start_time").unwrap(),
            "%H:%M:%S",
        )
        .unwrap();

        let end_date = NaiveDate::parse_from_str(
            args.get_one::<String>("end_date").unwrap(),
            "%Y-%m-%d",
        )
        .unwrap();
        let end_time = NaiveTime::parse_from_str(
            args.get_one::<String>("end_time").unwrap(),
            "%H:%M:%S",
        )
        .unwrap();

        let auction = Auction::new(
            item_descr,
            start_date,
            start_time,
            end_date,
            end_time,
            starting_bid,
            context.client.linked_name(),
        );
        let auction_id = Self::new_prefixed_id(&AUCTION_PREFIX.to_string());
        let auction_json = serde_json::to_string(&auction).unwrap();

        match context
            .client
            .set_data(auction_id.clone(), AUCTION_PREFIX.to_owned(), auction_json, None, None, false)
            .await
        {
            Ok(_) => Ok(Some(String::from(format!("Successfully created auction with id {}", &auction_id)))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create auction: {}",
                err.to_string()
            )))),
        }
    }

    // called by buyer
    pub async fn submit_bid(
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
            .set_data(bid_id.clone(), BID_PREFIX.to_owned(), bid_json, None, None, false)
            .await
        {
            Ok(_) => Ok(Some(String::from(format!("Successfully created bid with id {}", &bid_id)))),
            Err(err) => Ok(Some(String::from(format!(
                "Could not create bid: {}",
                err.to_string()
            )))),
        }
    }

    // called by auctioneer
    pub async fn apply_bid(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let bid_id = args.get_one::<String>("bid_id").unwrap().to_string();
        let bid_data_opt = context.client.get_data(&bid_id).await.unwrap();
        if bid_data_opt.is_none() {
            return Ok(Some(String::from(format!(
                "Bid with id {} does not exist",
                bid_id
            ))));
        }
        let bid: Bid =
            serde_json::from_str(bid_data_opt.unwrap().data_val()).unwrap();

        let auction_id = bid.auction_id.clone();
        let auction_data_opt =
            context.client.get_data(&auction_id).await.unwrap();
        if auction_data_opt.is_none() {
            return Ok(Some(String::from(format!(
                "Auction with id {} does not exist",
                auction_id
            ))));
        }
        let mut auction: Auction =
            serde_json::from_str(auction_data_opt.unwrap().data_val()).unwrap();

        if auction.apply_bid(&bid) {
            let auction_json = serde_json::to_string(&auction).unwrap();
            match context
                .client
                .set_data(
                    auction_id,
                    AUCTION_PREFIX.to_string(),
                    auction_json,
                    None,
                    None,
                    false,
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
            Ok(Some(String::from("Invalid bid")))
        }
    }

    // called by auctioneer
    pub async fn announce_sale(
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

        let auction_data_opt =
            context.client.get_data(&auction_id).await.unwrap();
        if auction_data_opt.is_none() {
            return Ok(Some(String::from(format!(
                "Auction with id {} does not exist",
                auction_id
            ))));
        }
        let mut auction: Auction =
            serde_json::from_str(auction_data_opt.unwrap().data_val()).unwrap();

        if auction.is_over() {
            auction.update_sale();
            let auction_json = serde_json::to_string(&auction).unwrap();
            match context
                .client
                .set_data(
                    auction_id,
                    AUCTION_PREFIX.to_string(),
                    auction_json,
                    None,
                    None,
                    false,
                )
                .await
            {
                // TODO if sold_bidder + sold_price == Some -> .unwrap()
                Ok(_) => Ok(Some(String::from(format!(
                    "Auction is over, sold to client {:?} for ${:?}",
                    auction.sold_bidder, auction.sold_price
                )))),
                Err(err) => Ok(Some(String::from(format!(
                    "Could not update auction: {}",
                    err.to_string()
                )))),
            }
        } else {
            Ok(Some(String::from(
                "Auction is not over, cannot announce sale.",
            )))
        }
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(AuctioningApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Auctioning App")
        .with_version("v0.1.0")
        .with_description("Scuba online auctioning app")
        .with_command_async(
            Command::new("init_new_device"),
            |_, context| Box::pin(AuctioningApp::init_new_device(context)),
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
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(AuctioningApp::add_contact(args, context)),
        )
        .with_command_async(
            Command::new("get_linked_devices"),
            |_, context| {
                Box::pin(AuctioningApp::get_linked_devices(context))
            },
        )
        .with_command_async(
            Command::new("get_data").arg(Arg::new("id").required(false)),
            |args, context| Box::pin(AuctioningApp::get_data(args, context)),
        )
        .with_command_async(
            Command::new("get_perms").arg(Arg::new("id").required(false)),
            |args, context| Box::pin(AuctioningApp::get_perms(args, context)),
        )
        .with_command_async(
            Command::new("get_groups").arg(Arg::new("id").required(false)),
            |args, context| Box::pin(AuctioningApp::get_groups(args, context)),
        )
        .with_command_async(
            Command::new("create_auction")
                .arg(
                    Arg::new("item_description")
                        .required(true)
                        .long("item-description")
                        .short('d'),
                )
                .arg(
                    Arg::new("starting_bid")
                        .required(true)
                        .long("starting-bid")
                        .short('b'),
                )
                .arg(
                    Arg::new("start_date")
                        .required(true)
                        .long("start-date")
                        .help("Format: YYYY-MM-DD"),
                )
                .arg(
                    Arg::new("start_time")
                        .required(true)
                        .long("start-time")
                        .help("Format: HH:MM:SS (24-hour)"),
                )
                .arg(
                    Arg::new("end_date")
                        .required(true)
                        .long("end-date")
                        .help("Format: YYYY-MM-DD"),
                )
                .arg(
                    Arg::new("end_time")
                        .required(true)
                        .long("end-time")
                        .help("Format: HH:MM:SS (24-hour)"),
                ),
            |args, context| {
                Box::pin(AuctioningApp::create_auction(args, context))
            },
        )
        .with_command_async(
            // TODO automatically share with auctioneer
            Command::new("submit_bid")
                .arg(
                    Arg::new("auction_id")
                        .required(true)
                        .long("auction_id")
                        .short('i'),
                )
                .arg(Arg::new("bid").required(true).long("bid").short('b')),
            |args, context| Box::pin(AuctioningApp::submit_bid(args, context)),
        )
        .with_command_async(
            Command::new("apply_bid").arg(
                Arg::new("bid_id").required(true),
            ),
            |args, context| Box::pin(AuctioningApp::apply_bid(args, context)),
        )
        .with_command_async(
            Command::new("announce_sale").arg(
                Arg::new("auction_id")
                    .required(true),
            ),
            |args, context| {
                Box::pin(AuctioningApp::announce_sale(args, context))
            },
        )
        .with_command_async(
            Command::new("add_to_open_auction_list").arg(
                Arg::new("client_id")
                    .required(true),
            ),
            |args, context| {
                Box::pin(AuctioningApp::add_to_open_auction_list(args, context))
            },
        )
        .with_command_async(
            Command::new("announce_open_auction").arg(
                Arg::new("auction_id")
                    .required(true),
            ),
            |args, context| {
                Box::pin(AuctioningApp::announce_open_auction(args, context))
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
            |args, context| Box::pin(AuctioningApp::share(args, context)),
        );

    repl.run_async().await
}
