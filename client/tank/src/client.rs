use async_condvar_fair::Condvar;
use async_recursion::async_recursion;
use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
//use std::collections::hash_map::Values;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;
use std::{thread, time};
use thiserror::Error;

use scuba_core::core::{Core, CoreClient, SequenceNumber};

use crate::data::{BasicData, ScubaData};
use crate::devices::Device;
use crate::metadata::{Group, PermType, PermissionSet};

/*
 * Existing set_*() functions whose writes should abide by consistency
 * rules:
 * - [x] create_standalone_device()
 * - [x] create_linked_device()
 * - [ ] delete_device_*() (when used)
 * - [x] add_contact()
 * - [x] add_permissions()
 * - [ ] delete_data() (not impl)
 * - [ ] remove_permissions() (not impl)
 */

/*
 * New get_*() functions whose reads should abide by consistency rules:
 * - [x] get_linked_devices()
 * - [x] get_data()
 * - [x] get_all_data()
 * - [x] get_perm()
 * - [x] get_all_perms()
 * - [x] get_group()
 * - [x] get_all_groups()
 *
 * TODO should Operations should be extended to include Get* calls for strict
 * serializability? Otherwise, read-only transactions will not be sent to the
 * server for ordering, which violates strict serializability
 *
 * TODO also need to modify TankClient configuration to only allow one transaction
 * at a time (for any transactional consistency models)
 */

#[derive(Debug, PartialEq, Error)]
pub enum Error {
    #[error("Device does not exist.")]
    UninitializedDevice,
    #[error("Operation violates data invariant.")]
    DataInvariantViolated,
    #[error("Sender ({0}) has insufficient permissions ({0}) for performing operation.")]
    InsufficientPermissions(String, String), /* TODO add more info to error
                                              * msg */
    #[error("Pending idkey {0} does not match sender idkey {0}")]
    PendingIdkeyMismatch(String, String),
    #[error("Permission set with id {0} already exists; cannot use SetPerm")]
    PermAlreadyExists(String),
    #[error("{0} is not a valid contact.")]
    InvalidContactName(String),
    #[error("Cannot add own device as contact.")]
    SelfIsInvalidContact,
    #[error("Data with id {0} does not exist.")]
    NonexistentData(String),
    #[error("Cannot convert {0} to string.")]
    StringConversionErr(String),
    #[error(transparent)]
    MetadataErr {
        #[from]
        source: crate::metadata::Error,
    },
    #[error(transparent)]
    DeviceErr {
        #[from]
        source: crate::devices::Error,
    },
    #[error("Received error while sending message: {0}.")]
    SendFailed(String),
    #[error("Invalid transaction status")]
    BadTransactionError,
    #[error("Transaction conflicts")]
    TransactionConflictsError,
    #[error("Transaction timed out")]
    Timeout,
    #[error("not an error")]
    SendToAll,
    #[error("also not an error")]
    NOOP,
    #[error("Tx not found locall")]
    TxNotFound,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum Operation {
    UpdateLinked(String, String, HashMap<String, Group>),
    ConfirmUpdateLinked(String, HashMap<String, Group>, HashMap<String, BasicData>),
    AddContact(String, String, HashMap<String, Group>),
    ConfirmAddContact(String, HashMap<String, Group>),
    SetPerm(String, PermissionSet),
    AddPermMembers(String, Option<String>, PermType),
    // TODO RemovePermMember
    SetGroup(String, Group),
    SetGroups(HashMap<String, Group>),
    //LinkGroups(String, String),
    //DeleteGroup(String),
    AddParent(String, String),
    //RemoveParent(String, String),
    //AddChild(String, String),
    //RemoveChild(String, String),
    UpdateData(String, BasicData),
    DeleteData(String),
    DeleteSelfDevice,
    DeleteOtherDevice(String),
    Test(String),
    Dummy(u64),
    //need to make sure these dont recurse
    TxStart(String, Transaction),
    TxCommit(String, SequenceNumber),
    TxAbort(String, SequenceNumber),
}

impl Operation {
    fn to_string(msg: &Operation) -> Result<String, serde_json::Error> {
        serde_json::to_string(msg)
    }

    fn from_string(msg: String) -> Result<Operation, serde_json::Error> {
        serde_json::from_str(msg.as_str())
    }

    fn get_data_id(operation: &Operation) -> String {
        match operation {
            Operation::UpdateData(data_id, _data_val) => data_id.to_string(),
            Operation::DeleteData(data_id) => data_id.to_string(),
            // TODO confirm this is never used
            _ => "".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Transaction {
    coordinator: String,
    recipients: Vec<String>,
    num_recipients: Option<usize>,
    ops: Vec<Operation>,
    prepare_sequence_number: Option<SequenceNumber>,
    commit_sequence_number: Option<SequenceNumber>,
    prev_seq_number: SequenceNumber,
    timeout: SequenceNumber,
    coord_loopback: bool,
}

impl Transaction {
    fn new(
        device_id: String,
        recipients: Vec<String>,
        ops: Vec<Operation>,
        prev_seq_number: SequenceNumber,
        coord_loopback: bool,
    ) -> Transaction {
        Transaction {
            coordinator: device_id,
            recipients: recipients.clone(),
            num_recipients: Some(recipients.len()),
            ops,
            prepare_sequence_number: None,
            commit_sequence_number: None,
            prev_seq_number,
            // TODO: make this dynamic? system dependent?
            timeout: 100000000000000000000000,
            coord_loopback,
        }
    }

    //fn to_string(msg: &Transaction) -> Result<String, serde_json::Error> {
    //    serde_json::to_string(msg)
    //}

    //fn from_string(msg: String) -> Result<Operation, serde_json::Error> {
    //    serde_json::from_str(msg.as_str())
    //}
}

pub struct TxCoordinator {
    seq_number: SequenceNumber,
    local_pending_tx: HashMap<SequenceNumber, (Vec<String>, Transaction)>,
    remote_pending_tx: HashMap<SequenceNumber, Transaction>,
    committed_tx: Vec<((SequenceNumber, SequenceNumber), Transaction)>,
    /* true if currently within transaction */
    tx_state: bool,
    temp_tx_ops: Vec<(Operation, Vec<String>)>,
}

// TODO: trim committed histories
impl TxCoordinator {
    fn new() -> TxCoordinator {
        TxCoordinator {
            seq_number: 0,
            tx_state: false,
            local_pending_tx: HashMap::new(),
            remote_pending_tx: HashMap::new(),
            committed_tx: Vec::new(),
            temp_tx_ops: Vec::new(),
        }
    }

    fn check_tx_state(&self) -> bool {
        return self.tx_state;
    }

    // TODO should this update go through the server?! maybe only from strict
    // serializability
    fn enter_tx(&mut self) -> bool {
        if self.tx_state {
            return false;
        }
        self.tx_state = true;
        return self.tx_state;
    }

    fn exit_tx(&mut self) -> (Vec<(Operation, Vec<String>)>, SequenceNumber) {
        let ops = self.temp_tx_ops.clone();
        self.temp_tx_ops = Vec::new();
        self.tx_state = false;
        return (ops, self.seq_number);
    }

    fn add_op_to_cur_tx(&mut self, msg: Operation, dst_idkeys: Vec<String>) {
        self.temp_tx_ops.push((msg, dst_idkeys));
    }

    fn get_transaction(&self, tx_id: SequenceNumber) -> Result<&Transaction, Error> {
        if self.local_pending_tx.contains_key(&tx_id) {
            return Ok(&self.local_pending_tx.get(&tx_id).unwrap().1);
        } else if self.remote_pending_tx.contains_key(&tx_id) {
            return Ok(&self.remote_pending_tx.get(&tx_id).unwrap());
        }
        return Err(Error::TxNotFound);
    }

    fn get_committed_transaction(
        &self,
        tx_id: SequenceNumber,
    ) -> Result<&Transaction, Error> {
        for ((start_seq, _), tx) in self.committed_tx.iter() {
            if *start_seq == tx_id {
                return Ok(&tx);
            }
        }
        return Err(Error::TxNotFound);
    }

    fn start_message(
        &mut self,
        my_device_id: String,
        sender: String,
        tx_id: SequenceNumber,
        unsequenced_tx: Transaction,
    ) -> Result<(), Error> {
        let mut tx = unsequenced_tx.clone();
        tx.prepare_sequence_number = Some(tx_id);

        if sender == my_device_id {
            self.local_pending_tx
                .insert(tx_id, (Vec::new(), tx.clone()));
        } else {
            self.remote_pending_tx.insert(tx_id, tx.clone());
        }

        let res = self.detect_conflict(&tx);
        if res {
            return Ok(());
        } else {
            return Err(Error::TransactionConflictsError);
        }
    }

    fn commit_message(
        &mut self,
        my_device_id: String,
        sender: String,
        tx_id: &SequenceNumber,
        seq: SequenceNumber, /* use seq number of commit message when
                              * committing */
    ) -> Result<(), Error> {
        // committing a tx i coordinated
        if sender == my_device_id {
            let q = &mut self.local_pending_tx;
            let mut tx = q[tx_id].1.clone();
            tx.commit_sequence_number = Some(seq);
            q.remove(tx_id);
            if seq > tx.prepare_sequence_number.unwrap_or_default() + tx.timeout {
                println!("TIMEOUT (coord = self)");
                println!("cur_seq: {}", &seq);
                println!(
                    "prep_seq: {}",
                    &tx.prepare_sequence_number.unwrap_or_default()
                );
                println!("to_offset: {}", &tx.timeout);
                println!(
                    "to_seq: {}",
                    tx.prepare_sequence_number.unwrap_or_default() + tx.timeout
                );
                println!(
                    "diff: {}",
                    tx.prepare_sequence_number.unwrap_or_default() + tx.timeout - seq
                );
                return Err(Error::Timeout);
            } else {
                self.committed_tx.push(((*tx_id, seq), tx));
            }
        // commit message for a remote transaction
        } else if self.remote_pending_tx.get(tx_id).unwrap().coordinator == sender {
            let mut tx = self.remote_pending_tx.remove(tx_id).unwrap();
            tx.commit_sequence_number = Some(seq);
            if seq > tx.prepare_sequence_number.unwrap_or_default() + tx.timeout {
                println!("TIMEOUT (coord = other)");
                println!("cur_seq: {}", &seq);
                println!(
                    "prep_seq: {}",
                    &tx.prepare_sequence_number.unwrap_or_default()
                );
                println!("to_offset: {}", &tx.timeout);
                println!(
                    "to_seq: {}",
                    tx.prepare_sequence_number.unwrap_or_default() + tx.timeout
                );
                println!(
                    "diff: {}",
                    tx.prepare_sequence_number.unwrap_or_default() - seq + tx.timeout
                );
                return Err(Error::Timeout);
            } else {
                self.committed_tx.push(((*tx_id, seq), tx));
            }
        }

        self.seq_number = *tx_id;
        Ok(())
    }

    fn abort_message(
        &mut self,
        my_device_id: String,
        sender: String,
        tx_id: &SequenceNumber,
    ) -> Result<(), Error> {
        // this client sent this abort message to all recipients
        if sender == my_device_id && self.local_pending_tx.contains_key(tx_id) {
            self.local_pending_tx.remove(tx_id);
        // this client got an abort message from a recipient
        // for a tx this client is coordinating
        } else if self.local_pending_tx.contains_key(tx_id) {
            assert!(self
                .remote_pending_tx
                .get(tx_id)
                .unwrap()
                .recipients
                .contains(&sender));
            return Err(Error::SendToAll);
        // a coordinator for a remote transaction is telling this client to
        // abort
        } else if self.remote_pending_tx.contains_key(tx_id) {
            assert_eq!(
                self.remote_pending_tx.get(tx_id).unwrap().coordinator,
                sender
            );
            self.remote_pending_tx.remove(tx_id);
        }
        Ok(())
    }

    fn detect_conflict(&self, msg: &Transaction) -> bool {
        let mut tx_keys = Vec::new();

        for op in msg.ops.clone().into_iter() {
            tx_keys.push(Operation::get_data_id(&op));
        }

        //impl for all enums -> move to func over two txs
        for tx in self.local_pending_tx.clone().into_iter() {
            for op in tx.1 .1.ops.clone().into_iter() {
                let data_id = Operation::get_data_id(&op);
                if tx_keys.contains(&data_id) {
                    return true;
                }
            }
        }

        for tx in self.remote_pending_tx.clone().into_iter() {
            for op in tx.1.ops.clone().into_iter() {
                let data_id = Operation::get_data_id(&op);
                if tx_keys.contains(&data_id) {
                    return true;
                }
            }
        }

        for tx in self.committed_tx.clone().into_iter() {
            //only iterate through committed transactions that were sequenced
            // after the prev txn accepted by the original client
            if tx.1.prepare_sequence_number.unwrap() > msg.prev_seq_number {
                for op in tx.1.ops.clone().into_iter() {
                    let data_id = Operation::get_data_id(&op);
                    if tx_keys.contains(&data_id) {
                        return true;
                    }
                }
            }
        }
        return false;
    }
}

#[derive(Clone)]
pub struct BenchArgs {
    core_benchmark_sends: usize,
    core_benchmark_recvs: usize,
    pub benchmark_runs: usize,
    bandwidth_filename: Option<String>,
    core_send_filename: String,
    core_recv_filename: String,
    send_filename: String,
    recv_update_filename: String,
    recv_dummy_filename: String,
}

impl BenchArgs {
    pub fn new(
        core_benchmark_sends: usize,
        core_benchmark_recvs: usize,
        benchmark_runs: usize,
        bandwidth_filename: Option<String>,
        core_send_filename: String,
        core_recv_filename: String,
        send_filename: String,
        recv_update_filename: String,
        recv_dummy_filename: String,
    ) -> BenchArgs {
        BenchArgs {
            core_benchmark_sends,
            core_benchmark_recvs,
            benchmark_runs,
            bandwidth_filename,
            core_send_filename,
            core_recv_filename,
            send_filename,
            recv_update_filename,
            recv_dummy_filename,
        }
    }
}

struct BenchFields {
    benchmark_send: usize,
    benchmark_recv_update: usize,
    benchmark_recv_dummy: usize,
    send_timestamp_vec: Vec<(usize, String, Instant)>,
    recv_update_timestamp_vec: Vec<(usize, String, Instant)>,
    recv_dummy_timestamp_vec: Vec<(usize, String, Instant)>,
    ctr_check_send: usize,
    ctr_check_recv_update: usize,
    ctr_check_recv_dummy: usize,
    send_filename: String,
    recv_update_filename: String,
    recv_dummy_filename: String,
}

impl BenchFields {
    pub fn new(args: BenchArgs) -> BenchFields {
        BenchFields {
            benchmark_send: args.benchmark_runs,
            benchmark_recv_update: args.benchmark_runs,
            benchmark_recv_dummy: args.benchmark_runs,
            send_timestamp_vec: Vec::<(usize, String, Instant)>::new(),
            recv_update_timestamp_vec: Vec::<(usize, String, Instant)>::new(),
            recv_dummy_timestamp_vec: Vec::<(usize, String, Instant)>::new(),
            ctr_check_send: 0,
            ctr_check_recv_update: 0,
            ctr_check_recv_dummy: 0,
            send_filename: args.send_filename,
            recv_update_filename: args.recv_update_filename,
            recv_dummy_filename: args.recv_dummy_filename,
        }
    }
}

#[derive(Clone)]
pub struct TankClient {
    core: Option<Arc<Core<TankClient>>>,
    // TODO remove pub
    pub device: Arc<RwLock<Option<Device<BasicData>>>>,
    ctr: Arc<Mutex<u64>>,
    ctr_cv: Arc<Condvar>,
    sec_wait_to_apply: Arc<Option<u64>>,
    block_writes: bool,
    sync_reads: bool,
    mult_outstanding: bool,
    //multikey: bool,
    tx_coordinator: Arc<RwLock<Option<TxCoordinator>>>,
    op_id_ctr: Arc<Mutex<(u64, HashSet<u64>)>>,
    op_id_ctr_cv: Arc<Condvar>,
    bench_fields: Arc<Mutex<Option<BenchFields>>>,
}

#[async_trait]
impl CoreClient for TankClient {
    async fn client_callback(
        &self,
        seq: SequenceNumber,
        sender: String,
        message: String,
        bench: bool,
    ) {
        let mut dummy = true;
        if message.contains("UpdateData") {
            dummy = false;
        }
        self.timestamp_recv(bench, dummy, "enter recvTANK").await;

        if self.sec_wait_to_apply.is_some() {
            thread::sleep(time::Duration::from_secs(self.sec_wait_to_apply.unwrap()));
        }

        match Operation::from_string(message.clone()) {
            Ok(operation) => {
                //if bench { println!("operation: {:?}", operation.clone()); }
                match self.check_permissions(&sender, &operation) {
                    Ok(_) => {
                        if self.validate_data_invariants(&operation) {
                            match self.demux(seq, operation).await {
                                Ok(_) => {}
                                Err(err) => {
                                    println!("Error in demux: {:?}", err)
                                }
                            }
                        } else {
                            println!(
                                "Error in validation: {:?}",
                                Error::DataInvariantViolated
                            );
                        }
                    }
                    Err(err) => println!("Error in permissions: {:?}", err),
                }
            }
            Err(_) => println!(
                "Error getting operation: {:?}",
                Error::StringConversionErr(message)
            ),
        };

        self.timestamp_inc_log_recv(bench, dummy, "exit recvTANK").await;

        let mut ctr = self.ctr.lock();
        if *ctr != 0 {
            //println!("cb_ctr: {:?}", *ctr);
            //println!("");
            *ctr -= 1;
            self.ctr_cv.notify_all();
        }
    }
}

impl TankClient {
    pub async fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        turn_encryption_off: bool,
        test_wait_num_callbacks: Option<u64>,
        sec_wait_to_apply: Option<u64>,
        // consistency args
        block_writes: bool,
        sync_reads: bool,
        mult_outstanding: bool,
        multikey: bool,
        bench_args: Option<BenchArgs>,
    ) -> TankClient {
        let ctr_val = test_wait_num_callbacks.unwrap_or(0);
        let tx_coordinator;
        if multikey {
            tx_coordinator = Arc::new(RwLock::new(Some(TxCoordinator::new())));
        } else {
            tx_coordinator = Arc::new(RwLock::new(None));
        }

        let bench_fields_opt;
        let bench_args_opt;
        if bench_args.is_some() {
            let bench_args_unwrapped = bench_args.unwrap();
            bench_fields_opt = Some(BenchFields::new(bench_args_unwrapped.clone()));
            bench_args_opt = Some(scuba_core::core::BenchArgs::new(
                bench_args_unwrapped.bandwidth_filename,
                bench_args_unwrapped.core_benchmark_sends,
                bench_args_unwrapped.core_benchmark_recvs,
                bench_args_unwrapped.core_send_filename,
                bench_args_unwrapped.core_recv_filename,
            ));
        } else {
            bench_fields_opt = None;
            bench_args_opt = None;
        }
        let bench_fields = Arc::new(Mutex::new(bench_fields_opt));

        let mut client = TankClient {
            core: None,
            device: Arc::new(RwLock::new(None)),
            ctr: Arc::new(Mutex::new(ctr_val)),
            ctr_cv: Arc::new(Condvar::new()),
            sec_wait_to_apply: Arc::new(sec_wait_to_apply),
            block_writes,
            sync_reads,
            mult_outstanding,
            tx_coordinator,
            op_id_ctr: Arc::new(Mutex::new((0, HashSet::new()))),
            op_id_ctr_cv: Arc::new(Condvar::new()),
            bench_fields,
        };

        let core = Core::new(
            ip_arg,
            port_arg,
            turn_encryption_off,
            Some(Arc::new(client.clone())),
            bench_args_opt,
        )
        .await;

        // At this point, if core was initialized with Some(Arc::new(client)),
        // then core points to a client _without an initialized core_. This
        // is a problem when the callback is called for the client, as it will
        // be called with an instance of the client (as &self) in
        // which core is None, and will not be able to use core (e.g.
        // to send messages, get its idkey, etc). Thus, _after_ the
        // client's core is initialized, a backpointer needs to be
        // added again to that client for which core is initialized.
        // The same goes for if core is initialized with None above,
        // although None may enable some interleavings that try to use core
        // before it is initialized altogether (e.g. the frontpointer, not
        // even the back pointer), so use Some(...) unless you add
        // another cv for this or something.
        client.core = Some(core.clone());
        core.set_client(Arc::new(client.clone())).await;
        client
    }

    /* Benchmarking code */

    // TODO move tank bmark fields into core and use core's timestamp_send impl
    // b/c currently they are identical except one is on tank and one is on core
    async fn timestamp_send(&self, bench: bool, msg: &'static str) {
        let bench_fields = &mut self.bench_fields.lock();
        if !bench || bench_fields.is_none() {
            return;
        }

        let bmark_send = bench_fields.as_ref().unwrap().benchmark_send.clone();
        bench_fields.as_mut().unwrap().send_timestamp_vec.push((
            bmark_send,
            String::from(msg),
            Instant::now(),
        ));
    }

    // FIXME diff from core b/c dummy + non-dummy
    async fn timestamp_recv(&self, bench: bool, dummy: bool, msg: &'static str) {
        let bench_fields = &mut self.bench_fields.lock();
        if !bench || bench_fields.is_none() {
            return;
        }

        if dummy {
            let bmark_recv = bench_fields.as_ref().unwrap().benchmark_recv_dummy.clone();
            bench_fields.as_mut().unwrap().recv_dummy_timestamp_vec.push((
                bmark_recv,
                String::from(msg),
                Instant::now(),
            ));
        } else {
            let bmark_recv = bench_fields.as_ref().unwrap().benchmark_recv_update.clone();
            bench_fields.as_mut().unwrap().recv_update_timestamp_vec.push((
                bmark_recv,
                String::from(msg),
                Instant::now(),
            ));
        }
    }

    async fn timestamp_inc_log_send(&self, bench: bool, msg: &'static str) {
        let bench_fields = &mut self.bench_fields.lock();
        if !bench || bench_fields.is_none() {
            return;
        }

        let bmark_send = bench_fields.as_ref().unwrap().benchmark_send.clone();
        // record timestamp
        bench_fields.as_mut().unwrap().send_timestamp_vec.push((
            bmark_send,
            String::from(msg),
            Instant::now(),
        ));

        // increment counter
        bench_fields.as_mut().unwrap().ctr_check_send += 1;
        let cur_count = bench_fields.as_ref().unwrap().benchmark_send;
        // log if done
        if cur_count == 1 {
            let mut f = File::options()
                .append(true)
                .create(true)
                .open(bench_fields.as_ref().unwrap().send_filename.clone())
                .unwrap();
            for entry in bench_fields.as_ref().unwrap().send_timestamp_vec.iter() {
                let _ = write!(f, "{:?}\n", entry);
            }
        } else if cur_count > 1 {
            bench_fields.as_mut().unwrap().benchmark_send -= 1;
        }
        //println!("dal ctr_check_send: {:?}", ctr_check_guard);
    }

    // FIXME diff from core b/c dummy + non-dummy
    async fn timestamp_inc_log_recv(&self, bench: bool, dummy: bool, msg: &'static str) {
        let bench_fields = &mut self.bench_fields.lock();
        if !bench || bench_fields.is_none() {
            return;
        }

        if dummy {
            let bmark_recv = bench_fields.as_ref().unwrap().benchmark_recv_dummy.clone();
            // record timestamp
            bench_fields.as_mut().unwrap().recv_dummy_timestamp_vec.push((
                bmark_recv,
                String::from(msg),
                Instant::now(),
            ));

            // increment counter
            bench_fields.as_mut().unwrap().ctr_check_recv_dummy += 1;
            let cur_count = bench_fields.as_ref().unwrap().benchmark_recv_dummy;
            // log if done
            if cur_count == 1 {
                let mut f = File::options()
                    .append(true)
                    .create(true)
                    .open(bench_fields.as_ref().unwrap().recv_dummy_filename.clone())
                    .unwrap();
                for entry in bench_fields.as_ref().unwrap().recv_dummy_timestamp_vec.iter() {
                    let _ = write!(f, "{:?}\n", entry);
                }
            } else if cur_count > 1 {
                bench_fields.as_mut().unwrap().benchmark_recv_dummy -= 1;
            }
            //println!("dal ctr_check_recv: {:?}", ctr_check_guard);
        } else {
            let bmark_recv = bench_fields.as_ref().unwrap().benchmark_recv_update.clone();
            // record timestamp
            bench_fields.as_mut().unwrap().recv_update_timestamp_vec.push((
                bmark_recv,
                String::from(msg),
                Instant::now(),
            ));

            // increment counter
            bench_fields.as_mut().unwrap().ctr_check_recv_update += 1;
            let cur_count = bench_fields.as_ref().unwrap().benchmark_recv_update;
            // log if done
            if cur_count == 1 {
                let mut f = File::options()
                    .append(true)
                    .create(true)
                    .open(bench_fields.as_ref().unwrap().recv_update_filename.clone())
                    .unwrap();
                for entry in bench_fields.as_ref().unwrap().recv_update_timestamp_vec.iter() {
                    let _ = write!(f, "{:?}\n", entry);
                }
            } else if cur_count > 1 {
                bench_fields.as_mut().unwrap().benchmark_recv_update -= 1;
            }
            //println!("dal ctr_check_recv: {:?}", ctr_check_guard);
        }
    }

    /* Transactions */

    fn discern_shards(
        &self,
        msg: Vec<(Operation, Vec<String>)>,
    ) -> BTreeMap<Vec<String>, (Vec<Operation>, bool)> {
        let mut txn_shards = BTreeMap::new();
        // put all ops going to the same set of recipients together
        for (op, mut recipients) in msg {
            recipients.sort();
            recipients.dedup();

            // add coordinator's idkey to txn if isn't already included so
            // the transaction can be committed
            let coord_loopback;
            if recipients.contains(&self.idkey()) {
                coord_loopback = false;
            } else {
                coord_loopback = true;
                recipients.push(self.idkey());
            }

            if txn_shards.contains_key(&recipients) {
                let (ops, _): &mut (Vec<Operation>, bool) =
                    txn_shards.get_mut(&recipients).unwrap();
                ops.push(op);
                //txn_shards.insert(recipients, ops);
            } else {
                txn_shards.insert(recipients, (vec![op], coord_loopback));
            }
        }

        return txn_shards;
    }

    async fn apply_locally(&self, tx_id: SequenceNumber) -> Result<(), Error> {
        let coord_guard = self.tx_coordinator.read();
        let tx = coord_guard
            .as_ref()
            .unwrap()
            .get_committed_transaction(tx_id)
            .unwrap();
        if tx.coord_loopback && self.idkey() == tx.coordinator {
            return Ok(());
        }
        for op in tx.ops.clone().into_iter() {
            //call into data store to apply
            // TODO: handle errors
            let _ = self.demux(tx_id.clone(), op).await;
        }
        Ok(())
    }

    async fn initiate_transaction(
        &self,
        device_id: String,
        ops: Vec<(Operation, Vec<String>)>,
        prev_seq_number: SequenceNumber,
    ) {
        let txn_shards = self.discern_shards(ops.clone());
        let mut txn_series = Vec::new();
        for (recipients, (ops, coord_loopback)) in txn_shards {
            let transaction = Transaction::new(
                device_id.clone(),
                recipients.clone(),
                ops,
                prev_seq_number,
                coord_loopback,
            );
            txn_series.push((
                recipients,
                Operation::to_string(&Operation::TxStart(device_id.clone(), transaction))
                    .unwrap(),
                false,
            ));
        }
        let _ = self.send_message(txn_series).await;
    }

    async fn send_abort_to_coordinator(&self, sender: String, tx_id: SequenceNumber) {
        let recipients = vec![self.idkey(), sender];
        let _ = self.send_message(vec![(
            recipients,
            Operation::to_string(&Operation::TxAbort(self.idkey(), tx_id)).unwrap(),
            false,
        )])
        .await;
    }

    async fn send_abort_as_coordinator(&self, tx_id: SequenceNumber) {
        let binding = self.tx_coordinator.read();
        let tx = binding
            .as_ref()
            .unwrap()
            .get_transaction(tx_id)
            .unwrap()
            .clone();
        let _ = self.send_message(vec![(
            tx.recipients,
            Operation::to_string(&Operation::TxAbort(self.idkey(), tx_id)).unwrap(),
            false,
        )])
        .await;
    }

    async fn send_commit_as_coordinator(&self, tx_id: SequenceNumber) {
        let binding = self.tx_coordinator.read();
        let tx = binding
            .as_ref()
            .unwrap()
            .get_transaction(tx_id)
            .unwrap()
            .clone();
        let _ = self.send_message(vec![(
            tx.recipients,
            Operation::to_string(&Operation::TxCommit(self.idkey(), tx_id)).unwrap(),
            false,
        )])
        .await;
    }

    /* Receiving-side functions */

    // TODO put each permission check in own function if will be used by
    // sending-side code too (which can then just call the respective func)
    fn check_permissions(
        &self,
        sender: &String,
        operation: &Operation,
    ) -> Result<(), Error> {
        match operation {
            /* Test op */
            Operation::Test(_msg) => Ok(()),
            /* Dummy op */
            Operation::Dummy(_num) => Ok(()),
            // TODO need manual checks
            //Operation::UpdateLinked(
            //    sender,
            //    temp_linked_name,
            //    members_to_add,
            //) => Ok(()),
            // TODO add perms to device groups
            //Operation::DeleteSelfDevice => Ok(()),
            //Operation::DeleteOtherDevice(idkey_to_delete) => Ok(()),
            // TODO add perms to device groups
            // TODO and need manual checks
            //Operation::AddContact => Ok(()),
            //Operation::ConfirmAddContact => Ok(()),
            /* Special case: use pending idkey */
            Operation::ConfirmUpdateLinked(_new_linked_name, _new_groups, _new_data) => {
                let pending_idkey_opt = self
                    .device
                    .read()
                    .as_ref()
                    .unwrap()
                    .get_pending_link_idkey();
                if pending_idkey_opt.is_none() {
                    return Err(Error::PendingIdkeyMismatch(
                        "None".to_string(),
                        sender.to_string(),
                    ));
                }
                let pending_idkey = pending_idkey_opt.unwrap();
                if pending_idkey != sender.to_string() {
                    return Err(Error::PendingIdkeyMismatch(
                        pending_idkey,
                        sender.to_string(),
                    ));
                }
                Ok(())
            }
            /* Need metadata-mod permissions */
            Operation::SetPerm(perm_id, _perm_val) => {
                // if permissions set with this id already exists, return error
                // this prevents malicious clients from replacing an existing
                // permissions object with one of their choosing, giving them
                // access to whatever data was pointed to
                if self
                    .device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .get_perm(perm_id)
                    .is_some()
                {
                    return Ok(()); //return Err(Error::PermAlreadyExists(perm_id.
                                   // to_string()));
                }
                Ok(())
            }
            Operation::AddPermMembers(perm_id, _group_id_opt, _new_members) => {
                // must have valid permissions given existing permissions
                // object
                if !self
                    .device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .has_metadata_mod_permissions(sender, perm_id, None)
                {
                    return Err(Error::InsufficientPermissions(
                        sender.to_string(),
                        perm_id.to_string(),
                    ));
                }
                Ok(())
            }
            /* Need metadata-mod permissions (via perm-backpointer) */
            Operation::SetGroup(group_id, group_val) => {
                // group can have any perm_id; if the perm object with perm_id
                // exists, then it is valid given the above checks run before
                // setting/modifying it and the sender thus must have the
                // required permissions; if the perm object does
                // not exist, false is returned
                if !self
                    .device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .find_metadata_mod_permissions(sender, group_val.clone())
                {
                    return Err(Error::InsufficientPermissions(
                        sender.to_string(),
                        group_id.to_string(),
                    ));
                }
                Ok(())
            }
            // FIXME groups may need to be sent in a particular order in order
            // for the above check to work in a loop
            Operation::SetGroups(_groups) => Ok(()),
            //Operation::LinkGroups(parent_id, child_id) => Ok(()),
            //Operation::DeleteGroup(group_id) => Ok(()),
            // TODO do permission check
            Operation::AddParent(_group_id, _parent_id) => Ok(()),
            //Operation::RemoveParent(group_id, parent_id) => Ok(()),
            //Operation::AddChild(group_id, child_id) => Ok(()),
            /* Need data-mod permissions */
            Operation::UpdateData(_data_id, _data_val) => {
                // FIXME need perm owner group when data is sent to a do-reader
                //if !self
                //    .device
                //    .read()
                //    .as_ref()
                //    .unwrap()
                //    .meta_store
                //    .read()
                //    .has_data_mod_permissions(sender, data_val.perm_id())
                //{
                //    return Err(Error::InsufficientPermissions(
                //        sender.to_string(),
                //        data_val.perm_id().to_string(),
                //    ));
                //}
                Ok(())
            }
            Operation::DeleteData(data_id) => {
                match self
                    .device
                    .read()
                    .as_ref()
                    .unwrap()
                    .data_store
                    .read()
                    .get_data(data_id)
                {
                    Some(data_val) => {
                        if !self
                            .device
                            .read()
                            .as_ref()
                            .unwrap()
                            .meta_store
                            .read()
                            .has_data_mod_permissions(sender, data_val.perm_id())
                        {
                            return Err(Error::InsufficientPermissions(
                                sender.to_string(),
                                data_val.perm_id().to_string(),
                            ));
                        }
                        Ok(())
                    }
                    // data doesn't exist, so continue (otherwise would
                    // get confusing error message)
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    fn validate_data_invariants(&self, operation: &Operation) -> bool {
        match operation {
            Operation::UpdateData(data_id, data_val) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .data_store
                .read()
                .validate(&data_id, &data_val),
            _ => true,
        }
    }

    #[async_recursion]
    async fn demux(
        &self,
        seq: SequenceNumber,
        operation: Operation,
    ) -> Result<(), Error> {
        match operation {
            Operation::UpdateLinked(sender, temp_linked_name, members_to_add) => self
                .update_linked_group(sender, temp_linked_name, members_to_add)
                .await
                .map_err(Error::from),
            Operation::ConfirmUpdateLinked(new_linked_name, new_groups, new_data) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .confirm_update_linked_group(new_linked_name, new_groups, new_data)
                .map_err(Error::from),
            Operation::AddContact(sender, contact_name, contact_devices) => self
                .add_contact_response(sender, contact_name, contact_devices)
                .await
                .map_err(Error::from),
            Operation::ConfirmAddContact(contact_name, contact_devices) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .add_contact(contact_name, contact_devices)
                .map_err(Error::from),
            Operation::SetPerm(perm_id, perm_val) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .write()
                    .set_perm(perm_id, perm_val);
                Ok(())
            }
            Operation::AddPermMembers(perm_id, group_id_opt, new_members) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .meta_store
                .write()
                .add_permissions(&perm_id, group_id_opt, new_members)
                .map_err(Error::from),
            Operation::SetGroup(group_id, group_val) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .write()
                    .set_group(group_id, group_val);
                Ok(())
            }
            Operation::SetGroups(groups) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .write()
                    .set_groups(groups);
                Ok(())
            }
            //Operation::LinkGroups(parent_id, child_id) => self
            //    .device
            //    .read()
            //    .as_ref()
            //    .unwrap()
            //    .meta_store
            //    .write()
            //    .link_groups(&parent_id, &child_id)
            //    .map_err(Error::from),
            //Operation::DeleteGroup(group_id) => {
            //    self.device
            //        .read()
            //        .as_ref()
            //        .unwrap()
            //        .meta_store
            //        .write()
            //        .delete_group(&group_id);
            //    Ok(())
            //}
            Operation::AddParent(group_id, parent_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .meta_store
                .write()
                .add_parent(&group_id, &parent_id)
                .map_err(Error::from),
            //Operation::RemoveParent(group_id, parent_id) => self
            //    .device
            //    .read()
            //    .as_ref()
            //    .unwrap()
            //    .meta_store
            //    .write()
            //    .remove_parent(&group_id, &parent_id)
            //    .map_err(Error::from),
            //Operation::AddChild(group_id, child_id) => self
            //    .device
            //    .read()
            //    .as_ref()
            //    .unwrap()
            //    .meta_store
            //    .write()
            //    .add_child(&group_id, &child_id)
            //    .map_err(Error::from),
            //Operation::RemoveChild(group_id, child_id) => self
            //    .device
            //    .read()
            //    .as_ref()
            //    .unwrap()
            //    .meta_store
            //    .write()
            //    .remove_child(&group_id, &child_id)
            //    .map_err(Error::from),
            Operation::UpdateData(data_id, data_val) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .data_store
                    .write()
                    .set_data(data_id, data_val);
                Ok(())
            }
            Operation::DeleteData(data_id) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .data_store
                    .write()
                    .delete_data(&data_id);
                Ok(())
            }
            Operation::DeleteSelfDevice => {
                let idkey = self.idkey().clone();
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .delete_device(idkey)
                    .map(|_| {
                        *self.device.write() = None;
                    })
                    .map_err(Error::from)
            }
            Operation::DeleteOtherDevice(idkey_to_delete) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .delete_device(idkey_to_delete)
                .map_err(Error::from),
            Operation::Test(_) => Ok(()),
            Operation::Dummy(op_id) => {
                let mut op_id_ctr = self.op_id_ctr.lock();
                // remove op_id from hashset
                op_id_ctr.1.remove(&op_id);
                self.op_id_ctr_cv.notify_all();
                core::mem::drop(op_id_ctr);
                Ok(())
            }
            Operation::TxStart(sender, tx) => {
                let res = self.tx_coordinator.write().as_mut().unwrap().start_message(
                    self.idkey(),
                    sender.clone(),
                    seq,
                    tx,
                );
                if res == Err(Error::TransactionConflictsError) {
                    self.send_abort_to_coordinator(sender, seq).await;
                } else {
                    if sender == self.idkey() {
                        self.send_commit_as_coordinator(seq).await;
                    }
                }
                Ok(())
            }
            Operation::TxCommit(sender, tx_id) => {
                let resp = self
                    .tx_coordinator
                    .write()
                    .as_mut()
                    .unwrap()
                    .commit_message(self.idkey(), sender, &tx_id, seq);
                if resp == Ok(()) {
                    let _ = self.apply_locally(tx_id).await;
                }
                Ok(())
            }
            Operation::TxAbort(sender, tx_id) => {
                let resp = self.tx_coordinator.write().as_mut().unwrap().abort_message(
                    self.idkey(),
                    sender,
                    &tx_id,
                );
                if resp == Err(Error::SendToAll) {
                    self.send_abort_as_coordinator(tx_id).await;
                }
                Ok(())
            }
        }
    }

    /* Sending-side functions */

    // TODO: change message to be a collection of messages
    async fn send_message(
        &self,
        series: Vec<(Vec<String>, String, bool)>,
    ) -> reqwest::Result<reqwest::Response> {
        self.core.as_ref().unwrap().send_message(series).await
    }

    /* Remaining top-level functionality */

    // Doesn't make sense to sync this read, since the idkey is needed to send
    // the sync message anyway
    // TODO but maybe sync anyway
    pub fn idkey(&self) -> String {
        self.core.as_ref().unwrap().idkey()
    }

    // Also doesn't make sense to sync this read, since there's no way to
    // change this after a device is initialized (at the moment)
    // TODO but maybe sync anyway
    pub fn linked_name(&self) -> String {
        self.device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .clone()
    }

    /*
     * Creating/linking devices
     */

    // TODO if no linked devices, linked_name can just == idkey
    // only create linked_group if link devices
    pub async fn create_standalone_device(&self) -> Result<(), Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // create device
        *self.device.write() =
            Some(Device::new(self.core.as_ref().unwrap().idkey(), None, None));
        Ok(())
    }

    pub async fn create_linked_device(&self, idkey: String) -> Result<(), Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        *self.device.write() = Some(Device::new(
            self.core.as_ref().unwrap().idkey(),
            None,
            Some(idkey.clone()),
        ));

        let linked_name = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .clone();

        let linked_members_to_add = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_all_subgroups(&linked_name);

        match self
            .send_or_add_to_txn(
                vec![idkey],
                &Operation::UpdateLinked(
                    self.core.as_ref().unwrap().idkey(),
                    linked_name,
                    linked_members_to_add,
                ),
                false,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    async fn update_linked_group(
        &self,
        sender: String,
        temp_linked_name: String,
        members_to_add: HashMap<String, Group>,
    ) -> Result<(), Error> {
        let _ = self.device
            .read()
            .as_ref()
            .unwrap()
            .update_linked_group(temp_linked_name.clone(), members_to_add)
            .map_err(Error::from);
        let perm_linked_name = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .to_string();

        // send all groups and to new members
        // FIXME send_or_add_to_txn?
        match self
            .send_message(vec![(
                vec![sender],
                Operation::to_string(&Operation::ConfirmUpdateLinked(
                    perm_linked_name,
                    self.device
                        .read()
                        .as_ref()
                        .unwrap()
                        .meta_store
                        .read()
                        .get_all_groups()
                        .clone(),
                    self.device
                        .read()
                        .as_ref()
                        .unwrap()
                        .data_store
                        .read()
                        .get_all_data()
                        .clone(),
                ))
                .unwrap(),
                false,
            )])
            .await
        {
            Ok(_) => {
                // TODO notify contacts of new members
                // get contacts

                Ok(())
            }
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    pub async fn get_linked_devices(&self) -> Result<HashSet<String>, Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        // FIXME send_or_add_to_txn?
        let res = self
            .send_message(vec![(
                vec![self.idkey()],
                Operation::to_string(&Operation::Dummy(op_id.clone())).unwrap(),
                false,
            )])
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // read linked devices
        Ok(self.device.read().as_ref().unwrap().linked_devices())
    }

    /*
     * Contacts
     */

    pub async fn add_contact(&self, contact_idkey: String) -> Result<(), Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        let linked_name = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .clone();

        // only add contact if contact_name is not a linked member
        if self
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .is_group_member(&contact_idkey, &linked_name, &None)
        {
            return Err(Error::SelfIsInvalidContact);
        }

        let linked_device_groups = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_all_subgroups(&linked_name);

        match self
            .send_or_add_to_txn(
                vec![contact_idkey],
                &Operation::AddContact(
                    self.core.as_ref().unwrap().idkey(),
                    linked_name,
                    linked_device_groups,
                ),
                false,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    // TODO user needs to accept first via, e.g., pop-up
    async fn add_contact_response(
        &self,
        sender: String,
        contact_name: String,
        contact_devices: HashMap<String, Group>,
    ) -> Result<(), Error> {
        let _ = self.device
            .read()
            .as_ref()
            .unwrap()
            .add_contact(contact_name.clone(), contact_devices)
            .map_err(Error::from);

        let linked_name = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .clone();
        let linked_device_groups = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_all_subgroups(&linked_name);

        // FIXME send_or_add_to_txn
        match self
            .send_message(vec![(
                vec![sender],
                Operation::to_string(&Operation::ConfirmAddContact(
                    linked_name,
                    linked_device_groups,
                ))
                .unwrap(),
                false,
            )])
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    /*
     * Deleting devices
     */

    pub async fn delete_self_device(&self) -> Result<(), Error> {
        // TODO send to contact devices too
        match self
            .send_message(vec![(
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .linked_devices_excluding_self(),
                Operation::to_string(&Operation::DeleteOtherDevice(
                    self.core.as_ref().unwrap().idkey(),
                ))
                .unwrap(),
                false,
            )])
            .await
        {
            Ok(_) => {
                // TODO wait for ACK that other devices have indeed received
                // above operations before deleting current device
                let idkey = self.core.as_ref().unwrap().idkey().clone();
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .delete_device(idkey)
                    .map(|_| *self.device.write() = None)
                    .map_err(Error::from)
            }
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    pub async fn delete_other_device(&self, to_delete: String) -> Result<(), Error> {
        // TODO send to contact devices too
        match self
            .send_message(vec![(
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .linked_devices_excluding_self_and_other(&to_delete),
                Operation::to_string(&Operation::DeleteOtherDevice(to_delete.clone()))
                    .unwrap(),
                false,
            )])
            .await
        {
            Ok(_) => {
                let _ = self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .delete_device(to_delete.clone())
                    .map_err(Error::from);

                match self
                    .send_message(vec![(
                        vec![to_delete.clone()],
                        Operation::to_string(&Operation::DeleteSelfDevice).unwrap(),
                        false,
                    )])
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(err) => Err(Error::SendFailed(err.to_string())),
                }
            }
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    pub async fn delete_all_devices(&self) -> Result<(), Error> {
        // TODO notify contacts

        match self
            .send_message(vec![(
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .linked_devices()
                    .iter()
                    .map(|x| x.clone())
                    .collect::<Vec<String>>(),
                Operation::to_string(&Operation::DeleteSelfDevice).unwrap(),
                false,
            )])
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    /*
     * Data
     */

    fn get_metadata_reader_groups_from_perm(
        &self,
        perm_val: &PermissionSet,
    ) -> Vec<String> {
        let mut vec = Vec::<String>::new();
        match perm_val.owners() {
            Some(owner_group) => vec.push(owner_group.to_string()),
            None => {}
        }
        match perm_val.writers() {
            Some(writer_group) => vec.push(writer_group.to_string()),
            None => {}
        }
        match perm_val.readers() {
            Some(reader_group) => vec.push(reader_group.to_string()),
            None => {}
        }
        vec
    }

    /* TODO MOVE THESE SOMEWHERE ELSE */

    pub fn start_transaction(&self) -> Result<(), Error> {
        let res = self.tx_coordinator.write().as_mut().unwrap().enter_tx();
        if !res {
            return Err(Error::BadTransactionError);
        }
        Ok(())
    }

    // TODO cancel transaction func?

    pub async fn end_transaction(&self) {
        let (ops, prev_seq_number) =
            self.tx_coordinator.write().as_mut().unwrap().exit_tx();
        self.initiate_transaction(self.idkey(), ops, prev_seq_number)
            .await;
    }

    async fn send_or_add_to_txn(
        &self,
        dst_idkeys: Vec<String>,
        op: &Operation,
        bench: bool,
    ) -> Result<(), Error> {
        if self.tx_coordinator.read().as_ref().is_some()
            && self
                .tx_coordinator
                .read()
                .as_ref()
                .unwrap()
                .check_tx_state()
        {
            self.tx_coordinator
                .write()
                .as_mut()
                .unwrap()
                .add_op_to_cur_tx(op.clone(), dst_idkeys.clone());
            Ok(())
        } else {
            match self
                .send_message(vec![(
                    dst_idkeys,
                    Operation::to_string(op).unwrap(),
                    bench,
                )])
                .await
            {
                Ok(_) => Ok(()),
                Err(err) => Err(Error::SendFailed(err.to_string())),
            }
        }
    }

    pub async fn get_perm(
        &self,
        perm_id: &String,
    ) -> Result<Option<PermissionSet>, Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // read perm
        let device_guard = self.device.read();
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let perm = meta_store_guard.get_perm(perm_id);
        Ok(perm.map(|x| x.clone()))
    }

    pub async fn get_all_perms(&self) -> Result<Vec<PermissionSet>, Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // read all perms
        let device_guard = self.device.read();
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let perms = meta_store_guard.get_all_perms().values();
        let mut values = Vec::<PermissionSet>::new();
        for perm in perms {
            values.push(perm.clone())
        }
        Ok(values)
    }

    pub async fn get_group(&self, group_id: &String) -> Result<Option<Group>, Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // read group
        let device_guard = self.device.read();
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let group = meta_store_guard.get_group(group_id);
        Ok(group.map(|x| x.clone()))
    }

    pub async fn get_all_groups(&self) -> Result<Vec<Group>, Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // read all groups
        let device_guard = self.device.read();
        let meta_store_guard = device_guard.as_ref().unwrap().meta_store.read();
        let groups = meta_store_guard.get_all_groups().values();
        let mut values = Vec::<Group>::new();
        for group in groups {
            values.push(group.clone())
        }
        Ok(values)
    }

    pub async fn get_data(&self, data_id: &String) -> Result<Option<BasicData>, Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // read data
        let device_guard = self.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let data = data_store_guard.get_data(data_id);
        Ok(data.map(|x| x.clone()))
    }

    pub async fn get_all_data(&self) -> Result<Vec<BasicData>, Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        /////////

        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                false,
            )
            .await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        /////////

        // check if need to block on reads, and if so, if this read has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.sync_reads && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        /////////

        // read all data
        let device_guard = self.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let data = data_store_guard.get_all_data().values();
        let mut values = Vec::<BasicData>::new();
        for datum in data {
            values.push(datum.clone())
        }
        Ok(values)
    }

    // TODO add facility for setting and sharing data at the same time

    pub async fn set_data(
        &self,
        data_id: String,
        data_type: String,
        data_val: String,
        // FIXME what is the below option for again? is it sharing upon
        // setting? why just enabled for data readers and, e.g., not
        // data writers?
        data_reader_idkeys: Option<Vec<String>>,
        add_perm_op_id: Option<u64>,
        bench: bool,
    ) -> Result<(), Error> {
        self.timestamp_send(bench, "enter sendTANK").await;

        let op_id;
        if add_perm_op_id.is_none() {
            // check if can have multiple outstanding ops, or if not, check that
            // no other ops are outstanding
            loop {
                let mut op_id_ctr = self.op_id_ctr.lock();
                if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                    // release the lock
                    let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
                } else {
                    // get op_id and inc id ctr
                    op_id = op_id_ctr.0;
                    op_id_ctr.0 += 1;
                    // add op into hashset
                    op_id_ctr.1.insert(op_id);
                    break;
                }
            }
        } else {
            op_id = add_perm_op_id.unwrap();
        }

        /////////

        // FIXME check write permissions

        let device_guard = self.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let existing_val = data_store_guard.get_data(&data_id).clone();

        // if data exists, use existing perms; otherwise create new one
        let perm_id;
        let mut perm_val;
        let mut device_ids: Vec<String>;
        match existing_val {
            Some(old_val) => {
                perm_id = old_val.perm_id().to_string();
                perm_val = device_guard
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .get_perm(&perm_id)
                    .unwrap()
                    .clone();

                // resolve idkeys
                if data_reader_idkeys.is_none() {
                    let group_ids = self.get_metadata_reader_groups_from_perm(&perm_val);
                    device_ids = device_guard
                        .as_ref()
                        .unwrap()
                        .meta_store
                        .read()
                        .resolve_group_ids(group_ids.iter().collect::<Vec<&String>>())
                        .into_iter()
                        .collect::<Vec<String>>();
                } else {
                    device_ids = data_reader_idkeys.unwrap();
                }
            }
            None => {
                // create new perms for this data_val
                perm_val = PermissionSet::new(None, None, None, None, None);
                perm_id = perm_val.perm_id().to_string();

                // create owner group that includes self.linked_name()
                let group_val = Group::new(
                    None,
                    Some(vec![perm_id.to_string()]),
                    false,
                    Some(Some(vec![self.linked_name().to_string()])),
                );

                // TODO add the new groups id as another parent of linked group
                // and send this update to all linked group members

                // add owner group into perms
                perm_val.set_owners(group_val.group_id());

                let idkeys = device_guard
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .resolve_group_ids(vec![&self.linked_name()])
                    .into_iter()
                    .collect::<Vec<String>>();

                // TODO why adding to device_ids when device_ids is the list
                // that metadata should be sent to?

                // resolve idkeys
                if data_reader_idkeys.is_none() {
                    // since group isn't stored yet, can't call
                    // resolve_group_ids() on it; however, we know that
                    // the only child of the new group is self.linked_name(),
                    // so just resolve that; and we happen to already
                    // have this computed anyway
                    device_ids = idkeys.clone();
                } else {
                    device_ids = data_reader_idkeys.unwrap();
                }

                // send perms
                let mut res = self
                    .send_or_add_to_txn(
                        idkeys.clone(),
                        &Operation::SetPerm(
                            perm_val.perm_id().to_string(),
                            perm_val.clone(),
                        ),
                        false,
                    )
                    .await;

                if res.is_err() {
                    return res;
                }

                // send group
                res = self
                    .send_or_add_to_txn(
                        idkeys.clone(),
                        &Operation::SetGroup(
                            group_val.group_id().to_string(),
                            group_val.clone(),
                        ),
                        false,
                    )
                    .await;

                if res.is_err() {
                    return res;
                }

                // add newly created group as a parent of linked_name
                res = self
                    .send_or_add_to_txn(
                        idkeys.clone(),
                        &Operation::AddParent(
                            self.linked_name().to_string(),
                            group_val.group_id().to_string(),
                        ),
                        false,
                    )
                    .await;

                if res.is_err() {
                    return res;
                }
            }
        }

        core::mem::drop(data_store_guard);

        let basic_data = BasicData::new(
            data_id.clone(),
            data_type.clone(),
            data_val,
            perm_id.clone(),
        );

        // add data-only-readers to device_ids
        match perm_val.do_readers() {
            Some(do_reader_group) => {
                // resolve group to idkeys, then push idkeys
                let group_ids = vec![do_reader_group.to_string()];
                let mut do_reader_ids = device_guard
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .resolve_group_ids(group_ids.iter().collect::<Vec<&String>>())
                    .into_iter()
                    .collect::<Vec<String>>();
                device_ids.append(&mut do_reader_ids);
            }
            None => {}
        }

        self.timestamp_send(bench, "enter CORE").await;

        let res = self
            .send_or_add_to_txn(
                // includes idkeys of _all_ permissions
                // (including data-only readers)
                // TODO Make separate for read-only members of txn
                device_ids.clone(),
                &Operation::UpdateData(data_id, basic_data),
                bench,
            )
            .await;

        self.timestamp_send(bench, "exit CORE").await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        self.timestamp_send(bench, "enter CORE").await;

        // FIXME better way to do this
        let res = self
            .send_or_add_to_txn(
                vec![self.idkey()],
                &Operation::Dummy(op_id.clone()),
                bench,
            )
            .await;

        self.timestamp_send(bench, "exit LASTCORE").await;

        if res.is_err() {
            return Err(Error::SendFailed(res.err().unwrap().to_string()));
        }

        self.timestamp_inc_log_send(bench, "exit sendTANK").await;

        ////////

        // check if need to block on writes, and if so, if this write has
        // returned from the server yet
        loop {
            let op_id_ctr = self.op_id_ctr.lock();
            if self.block_writes && op_id_ctr.1.contains(&op_id) {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                break;
            }
        }

        Ok(())
    }

    // TODO remove_data
    // + check write permissions

    // FIXME metadata GC: currently 1-to-1 mapping between data object
    // and permissions set; if collapse for space, then need a GC mechanism
    // when things are removed/changed. But for right now, we can leverage
    // this naive 1-to-1 mapping to simply delete a permissions set/any groups
    // when a sharing change removes them. Tradeoff space here

    // --nevermind--
    // will add children to groups via top_level_names if they exist,
    // otherwise idkeys (currently a linked group is created for every
    // device, so names should always be top_level_names unless this
    // changes). this makes it easier to maintain correct group membership
    // when the subset of devices for various top_level_names changes. so
    // names can be used to search for any existing groups that we can add
    // this piece of data to.
    // --nevermind--
    // FIXME but what about shared devices? If want to only share w a subset
    // of a user's devices, this precludes that

    // TODO also probably want a function that consolidates group names
    // before creating a new sharing group to remove duplicate names/members

    // FIXME writers should also have permissions...
    // contact-level flag could help with this, but also
    // could group->perm backpointers (go up the group tree
    // until find a perm_id OR propagate perm_ids to all group
    // children? - latter seems worse)

    pub async fn add_do_readers(
        &self,
        data_id: String,
        do_readers: Vec<&String>,
    ) -> Result<(), Error> {
        self.add_permissions(
            data_id,
            PermType::DOReaders(
                do_readers
                    .clone()
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<String>>(),
            ),
            do_readers,
        )
        .await
    }

    pub async fn add_readers(
        &self,
        data_id: String,
        readers: Vec<&String>,
    ) -> Result<(), Error> {
        self.add_permissions(
            data_id,
            PermType::Readers(
                readers
                    .clone()
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<String>>(),
            ),
            readers,
        )
        .await
    }

    pub async fn add_writers(
        &self,
        data_id: String,
        writers: Vec<&String>,
    ) -> Result<(), Error> {
        self.add_permissions(
            data_id,
            PermType::Writers(
                writers
                    .clone()
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<String>>(),
            ),
            writers,
        )
        .await
    }

    pub async fn add_permissions(
        &self,
        data_id: String,
        new_members: PermType,
        new_members_refs: Vec<&String>, // FIXME one or the other
    ) -> Result<(), Error> {
        // check if can have multiple outstanding ops, or if not, check that
        // no other ops are outstanding
        let op_id;
        loop {
            let mut op_id_ctr = self.op_id_ctr.lock();
            if !self.mult_outstanding && op_id_ctr.1.len() != 0 {
                // release the lock
                let _ = self.op_id_ctr_cv.wait(op_id_ctr).await;
            } else {
                // get op_id and inc id ctr
                op_id = op_id_ctr.0;
                op_id_ctr.0 += 1;
                // add op into hashset
                op_id_ctr.1.insert(op_id);
                break;
            }
        }

        let device_guard = self.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();

        // check that data exists
        match data_store_guard.get_data(&data_id) {
            None => return Err(Error::NonexistentData(data_id)),
            Some(data_val) => {
                let meta_store_guard =
                    device_guard.as_ref().unwrap().meta_store.read();

                let perm_val = meta_store_guard
                    .get_perm(data_val.perm_id())
                    .unwrap()
                    .clone();

                // if relevant group does not already exist, generate a
                // group_id for all devices to eventually use when creating
                // the group
                let group_id_opt = match new_members {
                    PermType::Owners(_) => match perm_val.owners() {
                        Some(_) => None,
                        None => Some(crate::metadata::generate_uuid()),
                    },
                    PermType::Writers(_) => match perm_val.writers() {
                        Some(_) => None,
                        None => Some(crate::metadata::generate_uuid()),
                    },
                    PermType::Readers(_) => match perm_val.readers() {
                        Some(_) => None,
                        None => Some(crate::metadata::generate_uuid()),
                    },
                    PermType::DOReaders(_) => match perm_val.do_readers() {
                        Some(_) => None,
                        None => Some(crate::metadata::generate_uuid()),
                    },
                };

                /*
                 * Collect all groups/ids that should receive the following
                 * metadata and data updates.
                 *
                 * TODO clean up into has_data_read() and has_metadata_read()
                 * functions to improve flexibility
                 */

                // existing owner
                let mut metadata_readers = Vec::<&String>::new();
                let mut data_readers = Vec::<&String>::new();
                // existing owners
                match perm_val.owners() {
                    Some(owners_group_id) => {
                        metadata_readers.push(owners_group_id);
                        data_readers.push(owners_group_id);
                    }
                    None => {}
                }
                // existing writers
                match perm_val.writers() {
                    Some(writers_group_id) => {
                        metadata_readers.push(writers_group_id);
                        data_readers.push(writers_group_id);
                    }
                    None => {}
                }
                // existing readers
                match perm_val.readers() {
                    Some(readers_group_id) => {
                        metadata_readers.push(readers_group_id);
                        data_readers.push(readers_group_id);
                    }
                    None => {}
                }
                // existing do-readers
                match perm_val.do_readers() {
                    Some(do_readers_group_id) => {
                        data_readers.push(do_readers_group_id);
                    }
                    None => {}
                }
                // PER ADDED PERM new member
                match new_members {
                    PermType::Owners(_) | PermType::Writers(_) | PermType::Readers(_) => {
                        metadata_readers.append(&mut new_members_refs.clone());
                        data_readers.append(&mut new_members_refs.clone());
                    }
                    PermType::DOReaders(_) => {
                        data_readers.append(&mut new_members_refs.clone());
                    }
                }

                // resolve the above to device ids
                let metadata_reader_idkeys = meta_store_guard
                    .resolve_group_ids(metadata_readers)
                    .into_iter()
                    .collect::<Vec<String>>();
                let data_reader_idkeys = meta_store_guard
                    .resolve_group_ids(data_readers)
                    .into_iter()
                    .collect::<Vec<String>>();

                // FIXME still need to send owner to data-only-readers so they
                // can confirm that the src idkey can actually write the data
                // object

                /*
                 * Then collect existing perm-associated groups.
                 * TODO for what?
                 */

                let mut assoc_groups = HashMap::<String, Group>::new();
                // get owner subgroups
                match perm_val.owners() {
                    Some(owners_group_id) => {
                        assoc_groups
                            .extend(meta_store_guard.get_all_subgroups(owners_group_id));
                    }
                    None => {}
                }
                // get writer subgroups
                match perm_val.writers() {
                    Some(writers_group_id) => {
                        assoc_groups
                            .extend(meta_store_guard.get_all_subgroups(writers_group_id));
                    }
                    None => {}
                }
                // get reader subgroups
                match perm_val.readers() {
                    Some(readers_group_id) => {
                        assoc_groups
                            .extend(meta_store_guard.get_all_subgroups(readers_group_id));
                    }
                    None => {}
                }
                // PER ADDED PERM get new-member subgroups
                for new_member_ref in &new_members_refs {
                    assoc_groups
                        .extend(meta_store_guard.get_all_subgroups(new_member_ref));
                }
                // PER ADDED PERM create new group if doesn't exist
                match group_id_opt.clone() {
                    Some(new_group_id) => match new_members.clone() {
                        PermType::Owners(new_members_vec)
                        | PermType::Writers(new_members_vec)
                        | PermType::Readers(new_members_vec) => {
                            let new_group = Group::new(
                                Some(new_group_id.clone()),
                                Some(vec![perm_val.perm_id().to_string()]),
                                false,
                                Some(Some(new_members_vec.clone())),
                            );
                            assoc_groups.insert(new_group_id, new_group);
                        }
                        PermType::DOReaders(_) => {}
                    },
                    None => {}
                }

                /*
                 * Wait to send messages until done reading from the
                 * meta_store b/c SetPerm/SetGroups/etc will need to take
                 * exclusive locks on it.
                 */

                core::mem::drop(meta_store_guard);

                // first send SetPerm for existing, unmodified perm_set
                // TODO remove this device from the idkeys for this op only
                let mut res = self
                    .send_or_add_to_txn(
                        metadata_reader_idkeys.clone(),
                        &Operation::SetPerm(
                            perm_val.perm_id().to_string(),
                            perm_val.clone(),
                        ),
                        false,
                    )
                    .await;

                if res.is_err() {
                    return res;
                }

                // then send SetGroups for all associated subgroups
                // FIXME will overwrite is_contact_name fields
                res = self
                    .send_or_add_to_txn(
                        metadata_reader_idkeys.clone(),
                        &Operation::SetGroups(assoc_groups),
                        false,
                    )
                    .await;

                if res.is_err() {
                    return res;
                }

                // PER ADDED PERM send AddPermMembers to all metadata-readers
                res = self
                    .send_or_add_to_txn(
                        metadata_reader_idkeys.clone(),
                        &Operation::AddPermMembers(
                            perm_val.perm_id().to_string(),
                            group_id_opt,
                            new_members,
                        ),
                        false,
                    )
                    .await;

                if res.is_err() {
                    return res;
                }

                // finally, send UpdateData (via set_data) to set the
                // associated data val:

                // copy out and drop read lock on data_store b/c UpdateData
                // will need exclusive lock to set data (on receive)

                let data_id = data_val.data_id().clone();
                let data_type = data_val.data_type().clone();
                let data_val_interior = data_val.data_val().clone();

                core::mem::drop(data_store_guard);
                core::mem::drop(device_guard);

                self.set_data(
                    data_id,
                    data_type,
                    data_val_interior,
                    Some(data_reader_idkeys),
                    Some(op_id),
                    false,
                )
                .await
            }
        }
    }

    // TODO unshare_data

    // TODO metadata_gc
}

// Expose JNI for android
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
pub mod android {
    extern crate jni;

    use self::jni::objects::JClass;
    use self::jni::sys::jlong;
    use self::jni::JNIEnv;
    use super::*;
    use tokio::runtime::Runtime;

    #[cfg(target_pointer_width = "64")]
    unsafe fn jlong_to_pointer<T>(val: jlong) -> *mut T {
        core::mem::transmute::<jlong, *mut T>(val)
    }

    #[cfg(target_pointer_width = "32")]
    unsafe fn jlong_to_pointer<T>(val: jlong) -> *mut T {
        core::mem::transmute::<u32, *mut T>(val as u32)
    }

    struct TankWithRuntime {
        tank: TankClient,
        runtime: Option<Runtime>,
    }

    impl TankWithRuntime {
        pub fn new(
        ) -> TankWithRuntime {
            let runtime = Runtime::new().unwrap();

            let mut tank_wrapper = runtime.block_on(async {
                Self {
                    tank: TankClient::new(
                        None, None, false, None, None, true, true, false,
                        false,
                        None, None, None, None, None, None, None, None, None,
                    ).await,
                    runtime: None,
                }
            });

            tank_wrapper.runtime = Some(runtime);

            tank_wrapper
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_example_scubaimport_TankWrapper_newTank(
        _: JNIEnv,
        _: JClass,
    ) -> jlong {
        let tank = TankWithRuntime::new();
        let boxed_tank: Box<TankWithRuntime> = Box::new(tank);
        let tank_ptr: *mut TankWithRuntime = Box::into_raw(boxed_tank);
        tank_ptr as jlong
    }
}

mod tests {
    //use crate::client::{Operation, TankClient};
    //use crate::data::ScubaData;

    #[tokio::test]
    async fn test_send_one_message() {
        let mut client_0 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_1 = TankClient::new(None, None, false, None, None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        println!("client_0 idkey = {:?}", client_0.idkey());
        println!("client_1 idkey = {:?}", client_1.idkey());

        // send operation
        let operation =
            Operation::to_string(&Operation::Test("hello".to_string())).unwrap();
        println!("sending operation to device 0");
        client_1
            .send_message(vec![client_0.idkey()], &operation)
            .await;

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }
    }

    #[tokio::test]
    async fn test_send_two_sequential_messages() {
        let mut client_0 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_1 = TankClient::new(None, None, false, Some(1), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        // send operation 1
        let operation_1 =
            Operation::to_string(&Operation::Test("hello".to_string())).unwrap();
        println!("sending operation to device 0");
        client_1
            .send_message(vec![client_0.idkey()], &operation_1)
            .await;

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        // send operation 2
        let operation_2 =
            Operation::to_string(&Operation::Test("goodbye".to_string())).unwrap();
        println!("sending operation to device 1");
        client_0
            .send_message(vec![client_1.idkey()], &operation_2)
            .await;

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }
    }

    #[tokio::test]
    async fn test_send_two_concurrent_messages() {
        let mut client_0 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_1 = TankClient::new(None, None, false, Some(1), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        // send operation 1
        let operation_1 =
            Operation::to_string(&Operation::Test("hello".to_string())).unwrap();
        println!("sending operation to device 0");
        client_1
            .send_message(vec![client_0.idkey()], &operation_1)
            .await;

        // send operation 2
        let operation_2 =
            Operation::to_string(&Operation::Test("goodbye".to_string())).unwrap();
        println!("sending operation to device 1");
        client_0
            .send_message(vec![client_1.idkey()], &operation_2)
            .await;

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }
    }

    #[tokio::test]
    async fn test_create_linked_device() {
        let mut client_0 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_1 = TankClient::new(None, None, false, Some(1), None).await;

        client_0.create_standalone_device();
        // sends operation to device 0 to link devices
        client_1.create_linked_device(client_0.idkey()).await;

        println!("client_0 idkey = {:?}", client_0.idkey());
        println!("client_1 idkey = {:?}", client_1.idkey());

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let linked_name_0: String = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .to_string();
        let linked_name_1: String = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .to_string();
        assert_eq!(linked_name_0, linked_name_1);
    }

    #[tokio::test]
    async fn test_serialization() {
        let mut client_0 = TankClient::new(None, None, false, Some(0), None).await;
        let mut client_1 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_2 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_3 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_4 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_5 = TankClient::new(None, None, false, Some(2), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();
        client_2.create_standalone_device();
        client_3.create_standalone_device();
        client_4.create_standalone_device();
        client_5.create_standalone_device();

        println!("client_0 idkey = {:?}", client_0.idkey());
        println!("client_1 idkey = {:?}", client_1.idkey());
        println!("client_2 idkey = {:?}", client_2.idkey());
        println!("client_3 idkey = {:?}", client_3.idkey());
        println!("client_4 idkey = {:?}", client_4.idkey());
        println!("client_5 idkey = {:?}", client_5.idkey());

        // construct a large message with many recipients
        // FIXME message size actually doesn't even matter because we .await
        // on send_message()
        let vec = vec![0x78; 1024];
        let operation_1 = Operation::to_string(&Operation::Test(
            std::str::from_utf8(vec.as_slice()).unwrap().to_string(),
        ))
        .unwrap();
        let recipients_1 = vec![
            client_1.idkey(),
            client_2.idkey(),
            client_3.idkey(),
            client_4.idkey(),
            client_5.idkey(),
        ];

        // construct a small message with a subset of recipients
        let operation_2 =
            Operation::to_string(&Operation::Test("small".to_string())).unwrap();
        let recipients_2 = vec![client_5.idkey()];

        // send the messages
        client_0.send_message(recipients_1, &operation_1).await;
        client_0.send_message(recipients_2, &operation_2).await;

        // client_0 loop is unnecessary
        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_2.ctr.lock();
            println!("ctr_2 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_2.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_3.ctr.lock();
            println!("ctr_3 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_3.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_4.ctr.lock();
            println!("ctr_4 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_4.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_5.ctr.lock();
            println!("ctr_5 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_5.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }
    }

    /*
    #[tokio::test]
    async fn test_add_contact() {
        let mut client_0 =
            TankClient::new(None, None, false, Some(1), None).await;
        let mut client_1 =
            TankClient::new(None, None, false, Some(1), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        client_0.add_contact(client_1.idkey()).await;

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        /* client_0 groups */

        let client_device_guard_0 = client_0.device.read();
        let linked_id_0: String = client_device_guard_0
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .to_string();
        let meta_store_guard_0 =
            client_device_guard_0.as_ref().unwrap().meta_store.read();
        let mut linked_group_0 = meta_store_guard_0
            .get_group_mut(&linked_id_0)
            .unwrap()
            .clone();
        linked_group_0.is_contact_name = true;

        let mut device_id_0 = &String::new();
        // have to iter b/c can't otherwise index into HashSet
        // (should only have one element)
        let children_0 = linked_group_0.children().as_ref().unwrap();
        assert!(!children_0.is_empty());
        for id in children_0.iter() {
            device_id_0 = id;
            break;
        }
        // drop mutable guard_0
        core::mem::drop(meta_store_guard_0);

        let meta_store_guard_0 =
            client_device_guard_0.as_ref().unwrap().meta_store.read();
        let device_group_0 =
            meta_store_guard_0.get_group(device_id_0).unwrap().clone();

        /* client_1 groups */

        let client_device_guard_1 = client_1.device.read();
        let linked_id_1: String = client_device_guard_1
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .to_string();
        let meta_store_guard_1 =
            client_device_guard_1.as_ref().unwrap().meta_store.read();
        let mut linked_group_1 = meta_store_guard_1
            .get_group_mut(&linked_id_1)
            .unwrap()
            .clone();
        linked_group_1.is_contact_name = true;

        let mut device_id_1 = &String::new();
        // have to iter b/c can't otherwise index into HashSet
        // (should only have one element)
        let children_1 = linked_group_1.children().as_ref().unwrap();
        assert!(!children_1.is_empty());
        for id in children_1.iter() {
            device_id_1 = id;
            break;
        }
        // drop mutable guard_1
        core::mem::drop(meta_store_guard_1);

        let meta_store_guard_1 =
            client_device_guard_1.as_ref().unwrap().meta_store.read();
        let device_group_1 =
            meta_store_guard_1.get_group(device_id_1).unwrap().clone();

        /* asserts */

        // check that clients have each others linked groups
        assert_eq!(
            meta_store_guard_0.get_group(&linked_id_1).unwrap(),
            &linked_group_1
        );
        assert_eq!(
            meta_store_guard_1.get_group(&linked_id_0).unwrap(),
            &linked_group_0
        );

        // check that clients have each others device groups
        assert_eq!(
            meta_store_guard_0.get_group(&device_id_1).unwrap(),
            &device_group_1
        );
        assert_eq!(
            meta_store_guard_1.get_group(&device_id_0).unwrap(),
            &device_group_0
        );
    }
    */

    // TODO test adding multiple contacts

    #[tokio::test]
    async fn test_get_all_contacts() {
        let mut client_0 = TankClient::new(None, None, false, Some(1), None).await;
        let mut client_1 = TankClient::new(None, None, false, Some(1), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        client_0.add_contact(client_1.idkey()).await;

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        assert_eq!(client_0.get_contacts().len(), 1);
        assert_eq!(client_1.get_contacts().len(), 1);
    }

    /*
    #[tokio::test]
    async fn test_delete_self_device() {
        let mut client_0 = TankClient::new(None, None, false).await;
        // upload otkeys to server
        client_0.core.receive_message().await;
        client_0.create_standalone_device();

        let mut client_1 = TankClient::new(None, None, false).await;
        // upload otkeys to server
        client_1.core.receive_message().await;

        // also sends operation to device 0 to link devices
        client_1.create_linked_device(client_0.idkey()).await;
        // receive update_linked...
        client_0.receive_operation().await;
        // receive update_linked... loopback
        client_1.receive_operation().await;
        // receive confirm_update_linked...
        client_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        client_0.receive_operation().await;

        // delete device
        client_0.delete_self_device().await;
        assert_eq!(client_0.device, None);

        // receive delete message
        println!(
            "client_1.device: {:#?}",
            client_1.device.as_ref().unwrap().meta_store.lock()
        );
        assert_eq!(client_1.device.as_ref().unwrap().linked_devices().len(), 2);
        client_1.receive_operation().await;
        println!(
            "client_1.device: {:#?}",
            client_1.device.as_ref().unwrap().meta_store.lock()
        );
        assert_eq!(client_1.device.as_ref().unwrap().linked_devices().len(), 1);
    }

    #[tokio::test]
    async fn test_delete_other_device() {
        let mut client_0 = TankClient::new(None, None, false).await;
        // upload otkeys to server
        client_0.core.receive_message().await;
        client_0.create_standalone_device();

        let mut client_1 = TankClient::new(None, None, false).await;
        // upload otkeys to server
        client_1.core.receive_message().await;

        // also sends operation to device 0 to link devices
        client_1.create_linked_device(client_0.idkey()).await;
        // receive update_linked...
        client_0.receive_operation().await;
        // receive update_linked... loopback
        client_1.receive_operation().await;
        // receive confirm_update_linked...
        client_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        client_0.receive_operation().await;

        // delete device
        println!(
            "client_0.device: {:#?}",
            client_0.device.read().as_ref().unwrap().meta_store.lock()
        );
        assert_eq!(client_0.device.read().as_ref().unwrap().linked_devices().len(), 2);
        client_0.delete_other_device(client_1.idkey().clone()).await;
        println!(
            "client_0.device: {:#?}",
            client_0.device.read().as_ref().unwrap().meta_store.lock()
        );
        assert_eq!(client_0.device.read().as_ref().unwrap().linked_devices().len(), 1);

        // receive delete operation
        client_1.receive_operation().await;
        assert_eq!(client_1.device.read(), None);
    }

    #[tokio::test]
    async fn test_delete_all_devices() {
        let mut client_0 = TankClient::new(None, None, false).await;
        // upload otkeys to server
        client_0.core.receive_message().await;
        client_0.create_standalone_device();

        let mut client_1 = TankClient::new(None, None, false).await;
        // upload otkeys to server
        client_1.core.receive_message().await;

        // also sends operation to device 0 to link devices
        client_1.create_linked_device(client_0.idkey()).await;
        // receive update_linked...
        client_0.receive_operation().await;
        // receive update_linked... loopback
        client_1.receive_operation().await;
        // receive confirm_update_linked...
        client_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        client_0.receive_operation().await;

        // delete all devices
        client_0.delete_all_devices().await;
        assert_ne!(client_0.device.read(), None);
        assert_ne!(client_1.device.read(), None);

        client_0.receive_operation().await;
        client_1.receive_operation().await;
        assert_eq!(client_0.device.read(), None);
        assert_eq!(client_1.device.read(), None);
        client_0.add_contact(client_1.idkey()).await;
    }
    */

    #[tokio::test]
    async fn test_set_data() {
        let mut client_0 = TankClient::new(None, None, false, Some(4), None).await;
        client_0.create_standalone_device();

        let data_type = "type".to_string();
        let data_id = crate::metadata::generate_uuid();
        let json_val = r#"{ data: true }"#.to_string();

        println!("");
        println!("BEFORE SETTING DATA");
        println!("");

        println!("client_0.idkey: {:?}", client_0.idkey());
        println!("client_0.linked_name: {:?}", client_0.linked_name());
        println!(
            "client_0.perms: {:?}",
            client_0
                .device
                .read()
                .as_ref()
                .unwrap()
                .meta_store
                .read()
                .get_all_perms()
        );
        let groups_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_all_groups()
            .clone();
        println!("client_0.groups:");
        for group in groups_0 {
            println!("--{:?}", group);
        }
        println!(
            "client_0.data: {:?}",
            client_0
                .device
                .read()
                .as_ref()
                .unwrap()
                .data_store
                .read()
                .get_all_data()
        );

        println!("");
        println!("SETTING DATA");
        println!("");

        let res = client_0
            .set_data(data_id.clone(), data_type.clone(), json_val.clone(), None)
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let data_val = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        println!("data_val: {:?}", data_val.clone());

        assert_eq!(*data_val.data_id(), data_id.clone());
        assert_eq!(*data_val.data_type(), data_type.clone());
        assert_eq!(*data_val.data_val(), json_val.clone());

        println!("");
        println!("CHECKING METADATA");
        println!("");

        println!("client_0.idkey: {:?}", client_0.idkey());
        println!("client_0.linked_name: {:?}", client_0.linked_name());
        println!(
            "client_0.perms: {:?}",
            client_0
                .device
                .read()
                .as_ref()
                .unwrap()
                .meta_store
                .read()
                .get_all_perms()
        );
        let groups_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_all_groups()
            .clone();
        println!("client_0.groups:");
        for group in groups_0 {
            println!("--{:?}", group);
        }
        println!(
            "client_0.data: {:?}",
            client_0
                .device
                .read()
                .as_ref()
                .unwrap()
                .data_store
                .read()
                .get_all_data()
        );

        let new_group_id = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val.perm_id())
            .unwrap()
            .owners()
            .as_ref()
            .unwrap()
            .clone();

        let linked_parents_list = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&client_0.linked_name())
            .unwrap()
            .parents()
            .clone();

        assert!(linked_parents_list.iter().any(|x| x == &new_group_id));
    }

    #[tokio::test]
    async fn test_add_writers() {
        let mut client_0 = TankClient::new(None, None, false, Some(8), None).await;
        let mut client_1 = TankClient::new(None, None, false, Some(5), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        let mut res = client_0.add_contact(client_1.idkey()).await;
        if res.is_err() {
            panic!("send failed");
        }

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 7 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 4 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        println!("");
        println!("CONTACTS ADDED");
        println!("");

        let data_type = "type".to_string();
        let data_id = crate::metadata::generate_uuid();
        let json_val = r#"{ data: true }"#.to_string();

        println!("");
        println!("BEFORE SETTING DATA");
        println!("");

        println!("client_0.idkey: {:?}", client_0.idkey());
        println!("client_0.linked_name: {:?}", client_0.linked_name());
        println!(
            "client_0.perms: {:?}",
            client_0
                .device
                .read()
                .as_ref()
                .unwrap()
                .meta_store
                .read()
                .get_all_perms()
        );
        let groups_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_all_groups()
            .clone();
        println!("client_0.groups:");
        for group in groups_0 {
            println!("--{:?}", group);
        }
        println!(
            "client_0.data: {:?}",
            client_0
                .device
                .read()
                .as_ref()
                .unwrap()
                .data_store
                .read()
                .get_all_data()
        );

        println!("");

        println!("client_1.idkey: {:?}", client_1.idkey());
        println!("client_1.linked_name: {:?}", client_1.linked_name());
        println!(
            "client_1.perms: {:?}",
            client_1
                .device
                .read()
                .as_ref()
                .unwrap()
                .meta_store
                .read()
                .get_all_perms()
        );
        let groups_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_all_groups()
            .clone();
        println!("client_1.groups:");
        for group in groups_1 {
            println!("--{:?}", group);
        }
        println!(
            "client_1.data: {:?}",
            client_1
                .device
                .read()
                .as_ref()
                .unwrap()
                .data_store
                .read()
                .get_all_data()
        );

        println!("");
        println!("SETTING DATA");
        println!("");

        res = client_0
            .set_data(data_id.clone(), data_type.clone(), json_val.clone(), None)
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 4 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        println!("");
        println!("SET DATA");
        println!("");

        let data_val = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        println!("data_val: {:?}", data_val.clone());

        assert_eq!(*data_val.data_id(), data_id.clone());
        assert_eq!(*data_val.data_type(), data_type.clone());
        assert_eq!(*data_val.data_val(), json_val.clone());

        let perm_id = data_val.perm_id();

        println!("");
        println!("");
        println!("ADDING WRITERS");

        res = client_0
            .add_writers(data_val.data_id().clone(), vec![&client_1.linked_name()])
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let data_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        let data_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        println!("data_val_0: {:?}", data_val_0);
        println!("data_val_1: {:?}", data_val_1);
        assert_eq!(data_val_0, data_val_1);

        let perm_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_0.perm_id())
            .unwrap()
            .clone();

        let perm_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_1.perm_id())
            .unwrap()
            .clone();

        println!("perm_val_0: {:?}", perm_val_0);
        println!("perm_val_1: {:?}", perm_val_1);
        assert_eq!(perm_val_0, perm_val_1);

        let writers_group_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_0.writers().as_ref().unwrap())
            .unwrap()
            .clone();

        let writers_group_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_1.writers().as_ref().unwrap())
            .unwrap()
            .clone();

        println!("writers_group_0: {:?}", writers_group_0);
        println!("writers_group_1: {:?}", writers_group_1);
        assert_eq!(writers_group_0, writers_group_1);

        // TODO try to change state
    }

    #[tokio::test]
    async fn test_add_readers() {
        let mut client_0 = TankClient::new(None, None, false, Some(10), None).await;
        let mut client_1 = TankClient::new(None, None, false, Some(7), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        let mut res = client_0.add_contact(client_1.idkey()).await;
        if res.is_err() {
            panic!("send failed");
        }

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 9 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 6 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let data_true = r#"{ data: true }"#;
        let data_false = r#"{ data: false }"#;

        let data_type = "type".to_string();
        let data_id = crate::metadata::generate_uuid();
        let json_val = data_true.to_string();
        res = client_0
            .set_data(data_id.clone(), data_type.clone(), json_val.clone(), None)
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 6 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let data_val = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        println!("data_val: {:?}", data_val.clone());

        assert_eq!(*data_val.data_id(), data_id.clone());
        assert_eq!(*data_val.data_type(), data_type.clone());
        assert_eq!(*data_val.data_val(), json_val.clone());

        let perm_id = data_val.perm_id();

        println!("");
        println!("");
        println!("ADDING READERS");

        res = client_0
            .add_readers(data_val.data_id().clone(), vec![&client_1.linked_name()])
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 2 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 2 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let data_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        let data_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        println!("data_val_0: {:?}", data_val_0);
        println!("data_val_1: {:?}", data_val_1);
        assert_eq!(data_val_0, data_val_1);

        let perm_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_0.perm_id())
            .unwrap()
            .clone();

        let perm_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_1.perm_id())
            .unwrap()
            .clone();

        println!("perm_val_0: {:?}", perm_val_0);
        println!("perm_val_1: {:?}", perm_val_1);
        assert_eq!(perm_val_0, perm_val_1);

        let readers_group_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_0.readers().as_ref().unwrap())
            .unwrap()
            .clone();

        let readers_group_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_1.readers().as_ref().unwrap())
            .unwrap()
            .clone();

        println!("readers_group_0: {:?}", readers_group_0);
        println!("readers_group_1: {:?}", readers_group_1);
        assert_eq!(readers_group_0, readers_group_1);

        /* have reader try to modify data */

        res = client_1
            .set_data(
                data_id.clone(),
                data_type.clone(),
                data_false.to_string(),
                None,
            )
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        println!("");
        println!("");
        println!("READER MODDING DATA");

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 1 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 1 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let data_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        let data_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        println!("data_val_0: {:?}", data_val_0);
        println!("data_val_1: {:?}", data_val_1);
        assert_eq!(data_val_0, data_val_1);

        let perm_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_0.perm_id())
            .unwrap()
            .clone();

        let perm_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_1.perm_id())
            .unwrap()
            .clone();

        println!("perm_val_0: {:?}", perm_val_0);
        println!("perm_val_1: {:?}", perm_val_1);
        assert_eq!(perm_val_0, perm_val_1);

        let readers_group_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_0.readers().as_ref().unwrap())
            .unwrap()
            .clone();

        let readers_group_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_1.readers().as_ref().unwrap())
            .unwrap()
            .clone();

        println!("readers_group_0: {:?}", readers_group_0);
        println!("readers_group_1: {:?}", readers_group_1);
        assert_eq!(readers_group_0, readers_group_1);

        /* now have owner modify data */

        res = client_0
            .set_data(
                data_id.clone(),
                data_type.clone(),
                data_false.to_string(),
                None,
            )
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        println!("");
        println!("");
        println!("OWNER MODDING DATA");

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_0.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        loop {
            let ctr = client_1.ctr.lock();
            println!("ctr_1 (test): {:?}", *ctr);
            if *ctr != 0 {
                let _ = client_1.ctr_cv.wait(ctr).await;
            } else {
                break;
            }
        }

        let data_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        let data_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .data_store
            .read()
            .get_data(&data_id)
            .unwrap()
            .clone();

        println!("data_val_0: {:?}", data_val_0);
        println!("data_val_1: {:?}", data_val_1);
        assert_eq!(data_val_0, data_val_1);

        let perm_val_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_0.perm_id())
            .unwrap()
            .clone();

        let perm_val_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_perm(data_val_1.perm_id())
            .unwrap()
            .clone();

        println!("perm_val_0: {:?}", perm_val_0);
        println!("perm_val_1: {:?}", perm_val_1);
        assert_eq!(perm_val_0, perm_val_1);

        let readers_group_0 = client_0
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_0.readers().as_ref().unwrap())
            .unwrap()
            .clone();

        let readers_group_1 = client_1
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
            .get_group(&perm_val_1.readers().as_ref().unwrap())
            .unwrap()
            .clone();

        println!("readers_group_0: {:?}", readers_group_0);
        println!("readers_group_1: {:?}", readers_group_1);
        assert_eq!(readers_group_0, readers_group_1);

        println!("client_0.idkey: {:?}", client_0.idkey());
        println!("client_0.linked_name: {:?}", client_0.linked_name());
        println!("client_1.idkey: {:?}", client_1.idkey());
        println!("client_1.linked_name: {:?}", client_1.linked_name());
    }
}
