use actix::{
    Actor, Addr, AsyncContext, Context, Handler, Message, MessageResponse, ResponseFuture,
};
use actix_web::{get, post, web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json;

const ACTOR_MAILBOX_CAP: usize = 1024;

#[derive(Message, Serialize, Deserialize, Clone, Debug)]
#[rtype(result = "SequencerRegisterResp")]
pub struct SequencerRegisterReq {
    pub base_url: String,
    pub inbox_count: u8,
    pub outbox_count: u8,
    pub isb_socket: Option<String>,
}

#[derive(MessageResponse, Serialize, Deserialize, Clone, Debug)]
pub struct SequencerRegisterResp {
    pub shard_id: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SequencerShardMap {
    pub shards: Vec<(String, Option<String>)>,
}

#[derive(MessageResponse, Clone, Debug)]
pub struct SequencerShardMapResp(Option<SequencerShardMap>);

#[derive(Message, Serialize, Deserialize, Clone, Debug)]
#[rtype(result = "()")]
pub struct EndEpochReq {
    pub shard_id: u8,
    pub epoch_id: u64,
    pub received_messages: usize,
}

#[derive(Message, Clone, Debug)]
#[rtype(result = "SequencerShardMapResp")]
pub struct SequencerShardMapReq;

#[derive(Message, Clone, Debug)]
#[rtype(result = "()")]
pub struct ProbeShards;

#[derive(Message, Clone, Debug)]
#[rtype(result = "()")]
pub struct EpochStart(u64);

#[derive(Message, Clone, Debug)]
#[rtype(result = "CurrentEpochResp")]
pub struct CurrentEpochReq;

#[derive(MessageResponse)]
pub struct CurrentEpochResp(u64);

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum EpochLogEntry {
    ShardEpochFinish(ShardEpochFinishLogEntry),
}

#[derive(Serialize, Clone, Debug)]
pub struct ShardEpochFinishLogEntry {
    shard_id: u8,
    epoch_id: u64,
    epoch_duration_us: u128,
    received_messages: usize,
}

enum Phase {
    Registration,
    Bootup,
    Sequencing(u64, std::time::Instant),
}

pub struct SequencerActor {
    num_shards: u8,
    shard_addresses: Vec<(bool, String, Option<String>)>,
    phase: Phase,
    client: reqwest::Client,
    epoch_log: std::fs::File,
}

impl SequencerActor {
    pub fn new(num_shards: u8) -> Self {
        println!("Starting SequencerActor");

        let epoch_log = std::fs::OpenOptions::new()
            .read(false)
            .write(true)
            .create_new(true)
            .open("./epoch_log.txt")
            .unwrap();

        SequencerActor {
            num_shards,
            shard_addresses: vec![],
            phase: Phase::Registration,
            client: reqwest::Client::new(),
            epoch_log,
        }
    }
}

impl Actor for SequencerActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(ACTOR_MAILBOX_CAP);
    }
}

impl Handler<SequencerRegisterReq> for SequencerActor {
    type Result = SequencerRegisterResp;

    fn handle(
        &mut self,
        msg: SequencerRegisterReq,
        ctx: &mut Context<Self>,
    ) -> SequencerRegisterResp {
        println!("Received SequencerRegisterReq");
        self.shard_addresses
            .push((false, msg.base_url, msg.isb_socket));

        if self.shard_addresses.len() == self.num_shards as usize {
            ctx.notify_later(ProbeShards, std::time::Duration::from_secs(1));
            println!("All shards registered, transitioning to bootup");
            self.phase = Phase::Bootup;
        }

        SequencerRegisterResp {
            shard_id: (self.shard_addresses.len() - 1) as u8,
        }
    }
}

impl Handler<ProbeShards> for SequencerActor {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, _msg: ProbeShards, ctx: &mut Context<Self>) -> Self::Result {
        let adds = self.shard_addresses.clone();
        let this = ctx.address().clone();
        let httpc = self.client.clone();

        Box::pin(async move {
            for (_, addr, _) in adds.iter() {
                if let Err(_) = httpc.get(addr).send().await {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    this.do_send(ProbeShards);
                    return;
                }
            }

            println!("All shards are reachable, initiating epoch 0");

            for (_, addr, _) in adds {
                println!("Requesting epoch start");
                httpc
                    .post(format!("{}/epoch/{}", addr, 0))
                    .send()
                    .await
                    .unwrap();
            }

            this.do_send(EpochStart(1));
        })
    }
}

impl Handler<EpochStart> for SequencerActor {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: EpochStart, _ctx: &mut Context<Self>) -> Self::Result {
        let EpochStart(epoch_id) = msg;
        self.phase = Phase::Sequencing(epoch_id, std::time::Instant::now());

        // println!("Start epoch {}", epoch_id);
        self.shard_addresses
            .iter_mut()
            .for_each(|(finished, _, _)| *finished = false);

        let adds = self.shard_addresses.clone();
        let httpc = self.client.clone();

        Box::pin(async move {
            for (_, addr, _) in adds {
                // TODO: parallelize
                // println!("Requesting epoch start");
                httpc
                    .post(format!("{}/epoch/{}", addr, epoch_id))
                    .send()
                    .await
                    .unwrap();
                // println!("Request done");
            }
        })
    }
}

impl Handler<CurrentEpochReq> for SequencerActor {
    type Result = CurrentEpochResp;

    fn handle(
        &mut self,
        _msg: CurrentEpochReq,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        CurrentEpochResp(match &self.phase {
            &Phase::Sequencing(epoch_id, _) => epoch_id,
            &Phase::Registration => 0,
            &Phase::Bootup => 0,
        })
    }
}

impl Handler<EndEpochReq> for SequencerActor {
    type Result = ();

    fn handle(&mut self, msg: EndEpochReq, ctx: &mut Context<Self>) -> Self::Result {
        let epoch_start_time = match &self.phase {
            Phase::Sequencing(epoch_id, start_time) => {
                assert!(*epoch_id == msg.epoch_id + 1);
                start_time
            }
            _ => panic!("Not yet sequencing"),
        };

        assert!(!self.shard_addresses[msg.shard_id as usize].0);
        self.shard_addresses[msg.shard_id as usize].0 = true;

        // Log that this shard has finished its epoch:
        use std::io::Write;
        serde_json::to_writer(
            &mut self.epoch_log,
            &EpochLogEntry::ShardEpochFinish(ShardEpochFinishLogEntry {
                shard_id: msg.shard_id,
                epoch_id: msg.epoch_id,
                epoch_duration_us: std::time::Instant::now()
                    .duration_since(*epoch_start_time)
                    .as_micros(),
                received_messages: msg.received_messages,
            }),
        )
        .unwrap();
        // TODO: buffered writer here?
        self.epoch_log.write(b"\n").unwrap();
        self.epoch_log.flush().unwrap();

        if self
            .shard_addresses
            .iter()
            .find(|(finished, _, _)| !finished)
            .is_none()
        {
            ctx.notify(EpochStart(msg.epoch_id + 2));
        }
    }
}

impl Handler<SequencerShardMapReq> for SequencerActor {
    type Result = SequencerShardMapResp;

    fn handle(
        &mut self,
        _msg: SequencerShardMapReq,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        if let Phase::Registration = self.phase {
            SequencerShardMapResp(None)
        } else {
            SequencerShardMapResp(Some(SequencerShardMap {
                shards: self
                    .shard_addresses
                    .iter()
                    .map(|(_, addr, isb_socket)| (addr.clone(), isb_socket.clone()))
                    .collect(),
            }))
        }
    }
}

#[post("/register")]
async fn register(
    state: web::Data<Addr<SequencerActor>>,
    req: web::Json<SequencerRegisterReq>,
) -> impl Responder {
    web::Json::<SequencerRegisterResp>(state.send(req.into_inner()).await.unwrap())
}

#[get("/shard-map")]
async fn shard_map(state: web::Data<Addr<SequencerActor>>) -> impl Responder {
    if let Some(shard_map) = state.send(SequencerShardMapReq).await.unwrap().0 {
        HttpResponse::Ok().json(shard_map)
    } else {
        HttpResponse::PaymentRequired().finish()
    }
}

#[post("/end-epoch")]
async fn end_epoch(
    state: web::Data<Addr<SequencerActor>>,
    req: web::Json<EndEpochReq>,
) -> impl Responder {
    state.send(req.into_inner()).await.unwrap();
    ""
}

#[get("/current-epoch")]
async fn current_epoch(state: web::Data<Addr<SequencerActor>>) -> impl Responder {
    format!("{}", state.send(CurrentEpochReq).await.unwrap().0)
}

pub async fn init(
    num_shards: u8,
) -> impl Fn(&mut web::ServiceConfig) + Clone + Send + 'static {
    let sequencer_addr = SequencerActor::new(num_shards).start();

    Box::new(move |service_config: &mut web::ServiceConfig| {
        service_config
            .app_data(web::Data::new(sequencer_addr.clone()))
            .service(register)
            .service(shard_map)
            .service(end_epoch)
            .service(current_epoch);
    })
}
