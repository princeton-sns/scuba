use reedline_repl_rs::clap::{Arg, ArgAction, ArgMatches, Command};
use reedline_repl_rs::Repl;
use reedline_repl_rs::Result as ReplResult;
use single_key_dal::client::NoiseKVClient;
use single_key_dal::data::NoiseData;
use std::sync::Arc;
use uuid::Uuid;

use serde::{Deserialize, Serialize};
use std::collections::LinkedList;

const GAME_PREFIX: &str = "game";
const BLACK: &str = "b";
const WHITE: &str = "w";
const TIE: &str = "tie";
const IN_PROGRESS: &str = "in progress";

// TODO Piece enum?

// 32 total
const EMPTY: &str = "";
// 8 per team (16 total)
const B_PAWN: &str = "b_pawn";
const W_PAWN: &str = "w_pawn";
// 2 per team (4 total)
const B_ROOK: &str = "b_rook";
const W_ROOK: &str = "w_rook";
// 2 per team (4 total)
const B_KNIGHT: &str = "b_knight";
const W_KNIGHT: &str = "w_knight";
// 2 per team (4 total)
const B_BISHOP: &str = "b_bishop";
const W_BISHOP: &str = "w_bishop";
// 1 per team (2 total)
const B_QUEEN: &str = "b_queen";
const W_QUEEN: &str = "w_queen";
// 1 per team (2 total)
const B_KING: &str = "b_king";
const W_KING: &str = "w_king";

// TODO Use refinement types for the coordinates
// https://docs.rs/refinement/latest/refinement/struct.Refinement.html
#[derive(Serialize, Deserialize)]
struct Move<'a> {
    piece: &'a str,
    start_x: usize, // 0-7
    start_y: usize, // 0-7
    end_x: usize,   // 0-7
    end_y: usize,   // 0-7
}

impl<'a> Move<'a> {
    pub fn new(
        piece: &'a str,
        start_x: usize,
        start_y: usize,
        end_x: usize,
        end_y: usize,
    ) -> Move {
        Self {
            piece,
            start_x,
            start_y,
            end_x,
            end_y,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Board {
    board: [[String; 8]; 8],
}

impl Board {
    pub fn new() -> Board {
        let board = [
            [
                B_ROOK.to_string(),
                B_KNIGHT.to_string(),
                B_BISHOP.to_string(),
                B_QUEEN.to_string(),
                B_KING.to_string(),
                B_BISHOP.to_string(),
                B_KNIGHT.to_string(),
                B_ROOK.to_string(),
            ],
            [
                B_PAWN.to_string(),
                B_PAWN.to_string(),
                B_PAWN.to_string(),
                B_PAWN.to_string(),
                B_PAWN.to_string(),
                B_PAWN.to_string(),
                B_PAWN.to_string(),
                B_PAWN.to_string(),
            ],
            [
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
            ],
            [
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
            ],
            [
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
            ],
            [
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
                EMPTY.to_string(),
            ],
            [
                W_PAWN.to_string(),
                W_PAWN.to_string(),
                W_PAWN.to_string(),
                W_PAWN.to_string(),
                W_PAWN.to_string(),
                W_PAWN.to_string(),
                W_PAWN.to_string(),
                W_PAWN.to_string(),
            ],
            [
                W_ROOK.to_string(),
                W_KNIGHT.to_string(),
                W_BISHOP.to_string(),
                W_QUEEN.to_string(),
                W_KING.to_string(),
                W_BISHOP.to_string(),
                W_KNIGHT.to_string(),
                W_ROOK.to_string(),
            ],
        ];
        Self { board }
    }

    pub fn other(&mut self) {}
}

#[derive(Serialize, Deserialize)]
struct Game<'a> {
    turn: &'a str,
    board: Board,
    b_move_history: LinkedList<Move<'a>>,
    w_move_history: LinkedList<Move<'a>>,
    winner: &'a str,
    //TODO timer: Timer,
}

impl<'a> Game<'a> {
    pub fn new() -> Game<'a> {
        Self {
            turn: WHITE,
            board: Board::new(),
            b_move_history: LinkedList::<Move>::new(),
            w_move_history: LinkedList::<Move>::new(),
            winner: IN_PROGRESS,
        }
    }
}

#[derive(Clone)]
struct ChessApp {
    client: NoiseKVClient,
}

impl ChessApp {
    pub async fn new() -> ChessApp {
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

    pub fn create_new_device(
        _args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        context.client.create_standalone_device();
        Ok(Some(String::from("Standalone device created.")))
    }

    pub async fn create_linked_device(
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

    pub fn get_state(
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
            Some(val) => Ok(Some(String::from(format!("{}", val)))),
            None => Ok(Some(String::from(format!(
                "Datum with id {} does not exist.",
                id,
            )))),
        }
    }

    pub async fn create_game(
        args: ArgMatches,
        context: &mut Arc<Self>,
    ) -> ReplResult<Option<String>> {
        if !context.exists_device() {
            return Ok(Some(String::from(
                "Device does not exist, cannot run command.",
            )));
        }

        let opponent_id =
            args.get_one::<String>("opponent").unwrap().to_string();

        let mut game_id = GAME_PREFIX.to_owned();
        game_id.push_str("/");
        game_id.push_str(&Uuid::new_v4().to_string());
        let json_string = serde_json::to_string(&Game::new()).unwrap();

        let res = context
            .client
            .set_data(
                game_id.clone(),
                GAME_PREFIX.to_string(),
                json_string,
                None,
            )
            .await;

        if res.is_err() {
            return Ok(Some(String::from(format!(
                "Error adding game: {}",
                res.err().unwrap()
            ))));
        }

        match context
            .client
            .add_writers(game_id.clone(), vec![&opponent_id.clone()])
            .await
        {
            Ok(_) => Ok(Some(String::from(format!(
                "Created game with id {}",
                game_id
            )))),
            Err(err) => Ok(Some(String::from(format!(
                "Error adding opponent: {}",
                err
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

        Ok(Some(String::from(format!(
            "Successfully shared datum {}",
            id
        ))))
    }
}

#[tokio::main]
async fn main() -> ReplResult<()> {
    let app = Arc::new(ChessApp::new().await);

    let mut repl = Repl::new(app.clone())
        .with_name("Chess App")
        .with_version("v0.1.0")
        .with_description("Noise chess app")
        .with_command(
            Command::new("create_new_device"),
            ChessApp::create_new_device,
        )
        .with_command_async(
            Command::new("create_linked_device")
                .arg(Arg::new("idkey").required(true)),
            |args, context| {
                Box::pin(ChessApp::create_linked_device(args, context))
            },
        )
        .with_command(Command::new("check_device"), ChessApp::check_device)
        .with_command(Command::new("get_name"), ChessApp::get_name)
        .with_command(Command::new("get_idkey"), ChessApp::get_idkey)
        .with_command(Command::new("get_contacts"), ChessApp::get_contacts)
        .with_command_async(
            Command::new("add_contact").arg(Arg::new("idkey").required(true)),
            |args, context| Box::pin(ChessApp::add_contact(args, context)),
        )
        .with_command(
            Command::new("get_linked_devices"),
            ChessApp::get_linked_devices,
        )
        .with_command(Command::new("get_data"), ChessApp::get_data)
        .with_command(Command::new("get_perms"), ChessApp::get_perms)
        .with_command(Command::new("get_groups"), ChessApp::get_groups)
        .with_command(
            Command::new("get_state").arg(Arg::new("id").required(true)),
            ChessApp::get_state,
        )
        .with_command_async(
            Command::new("create_game")
                .arg(Arg::new("opponent").required(true)),
            |args, context| Box::pin(ChessApp::create_game(args, context)),
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
                ),
            // TODO can add data-only readers, too
            |args, context| Box::pin(ChessApp::share(args, context)),
        );

    repl.run_async().await
}
