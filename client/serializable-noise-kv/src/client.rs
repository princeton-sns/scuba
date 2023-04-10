use async_condvar_fair::Condvar;
use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::{thread, time};
use thiserror::Error;

use noise_core::core::{Core, CoreClient};

use crate::data::{BasicData, NoiseData};
use crate::devices::Device;
use crate::groups::Group;

#[derive(Debug, PartialEq, Error)]
pub enum Error {
    #[error("Device does not exist.")]
    UninitializedDevice,
    #[error("Operation violates data invariant.")]
    DataInvariantViolated,
    #[error("Insufficient permissions for performing operation.")]
    InsufficientPermissions,
    #[error("{0} is not a valid contact.")]
    InvalidContactName(String),
    #[error("Cannot add own device as contact.")]
    SelfIsInvalidContact,
    #[error("Data with id {0} does not exist.")]
    NonexistentData(String),
    #[error("Cannot convert {0} to string.")]
    StringConversionErr(String),
    #[error(transparent)]
    GroupErr {
        #[from]
        source: crate::groups::Error,
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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum Operation {
    UpdateLinked(String, String, HashMap<String, Group>),
    ConfirmUpdateLinked(
        String,
        HashMap<String, Group>,
        HashMap<String, BasicData>,
    ),
    AddContact(String, String, HashMap<String, Group>),
    ConfirmAddContact(String, HashMap<String, Group>),
    SetGroup(String, Group),
    LinkGroups(String, String),
    DeleteGroup(String),
    AddParent(String, String),
    RemoveParent(String, String),
    AddChild(String, String),
    RemoveChild(String, String), // FIXME may never be used
    UpdateData(String, BasicData),
    DeleteData(String),
    //  AddPermission,
    //  RemovePermission,
    DeleteSelfDevice,
    DeleteOtherDevice(String),
    Test(String),
    //need to make sure these dont recurse
    TxStart(String, Transaction),
    TxCommit(String, u64),
    TxAbort(String, u64),
}

impl Operation {
    fn to_string(msg: &Operation) -> Result<String, serde_json::Error> {
        serde_json::to_string(msg)
    }

    fn from_string(msg: String) -> Result<Operation, serde_json::Error> {
        serde_json::from_str(msg.as_str())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Transaction {
    coordinator: String,
    recipients: Vec<String>,
    num_recipients: Option<usize>,
    ops: Vec<Operation>,
    seq_num: Option<u64>,
    prev_seq_num: Option<u64>,
}

impl Transaction {
    fn new(
        device_id: String,
        recipients: Vec<String>,
        ops: Vec<Operation>,
    ) -> Transaction {
        Transaction {
            coordinator: device_id,
            recipients: recipients.clone(),
            num_recipients: Some(recipients.len()),
            ops,
            seq_num: None,
            prev_seq_num: None,
        }
    }

    fn to_string(msg: &Transaction) -> Result<String, serde_json::Error> {
        serde_json::to_string(msg)
    }

    fn from_string(msg: String) -> Result<Operation, serde_json::Error> {
        serde_json::from_str(msg.as_str())
    }
}

pub struct TxCoordinator {
    seq_number: u64,
    /* transactions managed by me */
    local_pending_tx: HashMap<u64, (Vec<String>, Transaction)>,
    /* transactions managed by others awaiting commit message */
    remote_pending_tx: HashMap<u64, Transaction>,
    /* transactions committed (may be unecessary) */
    committed_tx: Vec<(u64, Transaction)>,

    tx_state: bool,
    /* true if currently within transaction */
    temp_tx_ops: Vec<Operation>,
}

// TODO: trim committed histories
// pass in device id to know what is mine
// pass in seq number from message
// this may have to go in core?
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
    fn enter_tx(&mut self) -> bool {
        if self.tx_state {
            return false;
        }
        self.tx_state = true;
        return self.tx_state;
    }

    fn exit_tx(&mut self) -> Vec<Operation> {
        let ops = self.temp_tx_ops.clone();
        self.temp_tx_ops = Vec::new();
        self.tx_state = false;
        return ops;
    }

    fn add_op_to_cur_tx(&mut self, msg: Operation) {
        self.temp_tx_ops.push(msg);
    }

    fn start_message(&mut self, sender: String, tx: Transaction) {
        //TODO  pass client id
        let my_device_id = "id";
        //TODO pass in sequencer number;
        let tx_id: u64 = 1;

        if sender == my_device_id {
            self.local_pending_tx.insert(tx_id, (Vec::new(), tx));
        } else {
            self.remote_pending_tx.insert(tx_id, tx);
        }
    }

    fn commit_message(&mut self, sender: String, tx_id: &u64) {
        //TODO  pass client id
        let my_device_id = "id";

        // committing a tx i coordinated
        if sender == my_device_id {
            let q = &mut self.local_pending_tx;
            let tx = q[tx_id].1.clone();
            //TODO: call apply locally
            q.remove(tx_id);
            self.committed_tx.push((*tx_id, tx));

        //coordinating commit messages from not me
        } else if self.local_pending_tx.contains_key(tx_id) {
            let tuple = self.local_pending_tx[tx_id].clone();
            let mut new_tuple = tuple.clone();
            new_tuple.0.push(sender);

            self.local_pending_tx.insert(*tx_id, new_tuple);
            if tuple.0.len() == tuple.1.num_recipients.unwrap() {
                //TODO: send commit to all recipients including me
                return;
            }

        // commit message for a remote transaction
        } else if self.remote_pending_tx.get(tx_id).unwrap().coordinator
            == sender
        {
            //TODO: call apply locally
            let tx = self.remote_pending_tx.remove(tx_id).unwrap();
            self.committed_tx.push((*tx_id, tx));
        }
    }

    fn abort_message(&mut self, sender: String, tx_id: &u64) {
        //TODO  pass client id
        let my_device_id = "id";

        if sender == my_device_id {
            self.local_pending_tx.remove(tx_id);
            return;
        } else if self.local_pending_tx.contains_key(tx_id) {
            assert!(self
                .remote_pending_tx
                .get(tx_id)
                .unwrap()
                .recipients
                .contains(&sender));
            //send abort message to all recipients including me
        } else if self.remote_pending_tx.contains_key(tx_id) {
            assert_eq!(
                self.remote_pending_tx.get(tx_id).unwrap().coordinator,
                sender
            );
            self.remote_pending_tx.remove(tx_id);
        }
    }

    fn detect_conflict(&self, msg: &Transaction) -> bool {
        let mut tx_keys = Vec::new();

        for op in msg.ops.clone().into_iter() {
            if let Operation::UpdateData(data_id, _) = op {
                tx_keys.push(data_id);
            }
        }

        //impl for all enums -> move to func over two txs
        for tx in self.local_pending_tx.clone().into_iter() {
            for op in tx.1 .1.ops.clone().into_iter() {
                if let Operation::UpdateData(data_id, _) = op {
                    if tx_keys.contains(&data_id) {
                        return true;
                    }
                }
            }
        }

        for tx in self.remote_pending_tx.clone().into_iter() {
            for op in tx.1.ops.clone().into_iter() {
                if let Operation::UpdateData(data_id, _) = op {
                    if tx_keys.contains(&data_id) {
                        return true;
                    }
                }
            }
        }

        for tx in self.committed_tx.clone().into_iter() {
            //only iterate through committed transactions that were sequenced
            // after the prev txn accepted by the original client
            if tx.1.seq_num > msg.prev_seq_num {
                for op in tx.1.ops.clone().into_iter() {
                    if let Operation::UpdateData(data_id, _) = op {
                        if tx_keys.contains(&data_id) {
                            return true;
                        }
                    }
                }
            }
        }
        return false;
    }
}

#[derive(Clone)]
pub struct NoiseKVClient {
    core: Option<Arc<Core<NoiseKVClient>>>,
    pub device: Arc<RwLock<Option<Device<BasicData>>>>,
    ctr: Arc<Mutex<u32>>,
    ctr_cv: Arc<Condvar>,
    sec_wait_to_apply: Arc<Option<u64>>,
    tx_coordinator: Arc<RwLock<TxCoordinator>>,
}

#[async_trait]
impl CoreClient for NoiseKVClient {
    async fn client_callback(&self, sender: String, message: String) {
        if self.sec_wait_to_apply.is_some() {
            thread::sleep(time::Duration::from_secs(
                self.sec_wait_to_apply.unwrap(),
            ));
        }

        match Operation::from_string(message.clone()) {
            Ok(operation) => {
                match self.check_permissions(&sender, &operation) {
                    Ok(_) => {
                        if self.validate_data_invariants(&operation) {
                            match self.demux(operation).await {
                                Ok(_) => {}
                                Err(err) => panic!("Error in demux: {:?}", err),
                            }
                        } else {
                            panic!(
                                "Error in validation: {:?}",
                                Error::DataInvariantViolated
                            );
                        }
                    }
                    Err(err) => panic!("Error in permissions: {:?}", err),
                }
            }
            Err(_) => panic!(
                "Error getting operation: {:?}",
                Error::StringConversionErr(message)
            ),
        };

        let mut ctr = self.ctr.lock();
        if *ctr != 0 {
            *ctr -= 1;
            self.ctr_cv.notify_all();
        }
    }
}

impl NoiseKVClient {
    pub async fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        turn_encryption_off: bool,
        test_wait_num_callbacks: Option<u32>,
        sec_wait_to_apply: Option<u64>,
    ) -> NoiseKVClient {
        let ctr_val = test_wait_num_callbacks.unwrap_or(0);
        let mut client = NoiseKVClient {
            core: None,
            device: Arc::new(RwLock::new(None)),
            ctr: Arc::new(Mutex::new(ctr_val)),
            ctr_cv: Arc::new(Condvar::new()),
            sec_wait_to_apply: Arc::new(sec_wait_to_apply),
            tx_coordinator: Arc::new(RwLock::new(TxCoordinator::new())),
        };

        let core = Core::new(
            ip_arg,
            port_arg,
            turn_encryption_off,
            Some(Arc::new(client.clone())),
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

    //fn exists_device(&self) -> bool {
    //    match self.device.read().as_ref() {
    //        Some(_) => true,
    //        None => false,
    //    }
    //}

    pub fn idkey(&self) -> String {
        self.core.as_ref().unwrap().idkey()
    }

    pub fn linked_name(&self) -> String {
        self.device
            .read()
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .clone()
    }

    /* Sending-side function */

    async fn send_message(
        &self,
        dst_idkeys: Vec<String>,
        payload: &String,
    ) -> reqwest::Result<reqwest::Response> {
        self.core
            .as_ref()
            .unwrap()
            .send_message(dst_idkeys, payload)
            .await
    }

    /* Transactions */
    fn collect_recipients(&self, msg: Vec<Operation>) -> Vec<String> {
        let mut recipients = Vec::new();
        for op in msg {
            if let Operation::UpdateData(_, data) = op {
                let group_id = data.group_id();
                let device_ids = self
                    .device
                    .read() //resolve by moving into client
                    .as_ref()
                    .unwrap()
                    .group_store
                    .lock()
                    .resolve_ids(vec![&group_id])
                    .into_iter()
                    .collect::<Vec<String>>();

                recipients.extend(device_ids);
            }
        }

        recipients.sort();
        recipients.dedup();

        return recipients;
    }

    async fn apply_locally(msg: &Transaction) -> Result<(), Error> {
        for op in msg.ops.clone().into_iter() {
            //call into data store to apply
        }
        Ok(())
    }

    fn initiate_transaction(&mut self, device_id: String, ops: Vec<Operation>) {
        let recipients = self.collect_recipients(ops.clone());
        let transaction =
            Transaction::new(device_id.clone(), recipients.clone(), ops);
        self.send_message(
            recipients,
            &Operation::to_string(&Operation::TxStart(device_id, transaction))
                .unwrap(),
        );
    }

    /* Receiving-side functions */

    fn check_permissions(
        &self,
        _sender: &String,
        _operation: &Operation,
    ) -> Result<(), Error> {
        // TODO actually check permissions
        //match operation {
        //    Operation::UpdateLinked(
        //        sender,
        //        temp_linked_name,
        //        members_to_add,
        //    ) => Ok(()),
        //    Operation::ConfirmUpdateLinked(new_linked_name,
        // new_groups, new_data) => {        Ok(())
        //    }
        //    Operation::AddContact => Ok(()),
        //    Operation::ConfirmAddContact => Ok(()),
        //    Operation::SetGroup(group_id, group_val) => Ok(()),
        //    Operation::LinkGroups(parent_id, child_id) => Ok(()),
        //    Operation::DeleteGroup(group_id) => Ok(()),
        //    Operation::AddParent(group_id, parent_id) => Ok(()),
        //    Operation::RemoveParent(group_id, parent_id) =>
        // Ok(()),    Operation::AddChild(group_id,
        // child_id) => Ok(()),
        //    Operation::RemoveChild(group_id, child_id) => Ok(()),
        //    Operation::UpdateData(data_id, data_val) => Ok(()),
        //    Operation::DeleteData(data_id) => Ok(()),
        //    Operation::DeleteSelfDevice => Ok(()),
        //    Operation::DeleteOtherDevice(idkey_to_delete) =>
        // Ok(()),    Operation::Test(msg) => Ok(()),
        //}
        Ok(())
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

    async fn demux(&self, operation: Operation) -> Result<(), Error> {
        match operation {
            Operation::UpdateLinked(
                sender,
                temp_linked_name,
                members_to_add,
            ) => self
                .update_linked_group(sender, temp_linked_name, members_to_add)
                .await
                .map_err(Error::from),
            Operation::ConfirmUpdateLinked(
                new_linked_name,
                new_groups,
                new_data,
            ) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .confirm_update_linked_group(
                    new_linked_name,
                    new_groups,
                    new_data,
                )
                .map_err(Error::from),
            Operation::AddContact(sender, contact_name, contact_devices) => {
                self.add_contact_response(sender, contact_name, contact_devices)
                    .await
                    .map_err(Error::from)
            }
            Operation::ConfirmAddContact(contact_name, contact_devices) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .add_contact(contact_name, contact_devices)
                .map_err(Error::from),
            Operation::SetGroup(group_id, group_val) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .group_store
                    .lock()
                    .set_group(group_id, group_val);
                Ok(())
            }
            Operation::LinkGroups(parent_id, child_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .link_groups(&parent_id, &child_id)
                .map_err(Error::from),
            Operation::DeleteGroup(group_id) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .group_store
                    .lock()
                    .delete_group(&group_id);
                Ok(())
            }
            Operation::AddParent(group_id, parent_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .add_parent(&group_id, &parent_id)
                .map_err(Error::from),
            Operation::RemoveParent(group_id, parent_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .remove_parent(&group_id, &parent_id)
                .map_err(Error::from),
            Operation::AddChild(group_id, child_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .add_child(&group_id, &child_id)
                .map_err(Error::from),
            Operation::RemoveChild(group_id, child_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .remove_child(&group_id, &child_id)
                .map_err(Error::from),
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
            Operation::TxStart(sender, tx) => {
                self.tx_coordinator.write().start_message(sender, tx);
                Ok(()) //FIX return something more meaningful
            }
            Operation::TxCommit(sender, tx_id) => {
                self.tx_coordinator.write().commit_message(sender, &tx_id);
                Ok(())
            }
            Operation::TxAbort(sender, tx_id) => {
                self.tx_coordinator.write().abort_message(sender, &tx_id);
                Ok(())
            }
        }
    }

    /* Remaining top-level functionality */

    /*
     * Creating/linking devices
     */

    // TODO if no linked devices, linked_name can just == idkey
    // only create linked_group if link devices
    pub fn create_standalone_device(&self) {
        *self.device.write() =
            Some(Device::new(self.core.as_ref().unwrap().idkey(), None, None));
    }

    pub async fn create_linked_device(
        &self,
        idkey: String,
    ) -> Result<(), Error> {
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
            .group_store
            .lock()
            .get_all_subgroups(&linked_name);

        match self
            .send_message(
                vec![idkey],
                &Operation::to_string(&Operation::UpdateLinked(
                    self.core.as_ref().unwrap().idkey(),
                    linked_name,
                    linked_members_to_add,
                ))
                .unwrap(),
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
        self.device
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
        match self
            .send_message(
                vec![sender],
                &Operation::to_string(&Operation::ConfirmUpdateLinked(
                    perm_linked_name,
                    self.device
                        .read()
                        .as_ref()
                        .unwrap()
                        .group_store
                        .lock()
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
            )
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

    /*
     * Contacts
     */

    pub fn get_contacts(&self) -> HashSet<String> {
        self.device.read().as_ref().unwrap().get_contacts()
    }

    fn is_contact(&self, name: &String) -> bool {
        match self
            .device
            .read()
            .as_ref()
            .unwrap()
            .group_store
            .lock()
            .get_group(name)
        {
            Some(group_val) => group_val.is_contact_name,
            None => false,
        }
    }

    pub async fn add_contact(
        &self,
        contact_idkey: String,
    ) -> Result<(), Error> {
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
            .group_store
            .lock()
            .is_group_member(&contact_idkey, &linked_name)
        {
            return Err(Error::SelfIsInvalidContact);
        }

        let linked_device_groups = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .group_store
            .lock()
            .get_all_subgroups(&linked_name);

        match self
            .send_message(
                vec![contact_idkey],
                &Operation::to_string(&Operation::AddContact(
                    self.core.as_ref().unwrap().idkey(),
                    linked_name,
                    linked_device_groups,
                ))
                .unwrap(),
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
        self.device
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
            .group_store
            .lock()
            .get_all_subgroups(&linked_name);

        match self
            .send_message(
                vec![sender],
                &Operation::to_string(&Operation::ConfirmAddContact(
                    linked_name,
                    linked_device_groups,
                ))
                .unwrap(),
            )
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
            .send_message(
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .linked_devices_excluding_self(),
                &Operation::to_string(&Operation::DeleteOtherDevice(
                    self.core.as_ref().unwrap().idkey(),
                ))
                .unwrap(),
            )
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

    pub async fn delete_other_device(
        &self,
        to_delete: String,
    ) -> Result<(), Error> {
        // TODO send to contact devices too
        match self
            .send_message(
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .linked_devices_excluding_self_and_other(&to_delete),
                &Operation::to_string(&Operation::DeleteOtherDevice(
                    to_delete.clone(),
                ))
                .unwrap(),
            )
            .await
        {
            Ok(_) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .delete_device(to_delete.clone())
                    .map_err(Error::from);

                match self
                    .send_message(
                        vec![to_delete.clone()],
                        &Operation::to_string(&Operation::DeleteSelfDevice)
                            .unwrap(),
                    )
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
            .send_message(
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .linked_devices()
                    .iter()
                    .map(|x| x.clone())
                    .collect::<Vec<String>>(),
                &Operation::to_string(&Operation::DeleteSelfDevice).unwrap(),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
    }

    /*
     * Data
     */

    pub fn start_transaction(&mut self) -> Result<(), Error> {
        let res = self.tx_coordinator.write().enter_tx();
        if !res {
            return Err(Error::BadTransactionError);
        }
        Ok(())
    }

    pub fn end_transaction(&mut self) {
        let ops = self.tx_coordinator.write().exit_tx();
        self.initiate_transaction(self.idkey(), ops);
    }

    pub async fn set_data(
        &self,
        data_id: String,
        data_type: String,
        data_val: String,
        group_opt: Option<String>,
    ) -> Result<(), Error> {
        let device_guard = self.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let existing_val = data_store_guard.get_data(&data_id);

        // if group_opt = None, keep old group_id (if no old group_id, use
        // linked_name) if group_opt = Some, use new group_id
        let group_id;
        if group_opt.is_none() {
            match existing_val {
                Some(old_val) => group_id = old_val.group_id().to_string(),
                None => group_id = self.linked_name(),
            }
        } else {
            group_id = group_opt.unwrap();
        }

        // TODO call validate before sending? or only upon receipt?

        let basic_data = BasicData::new(
            data_id.clone(),
            data_type.clone(),
            data_val,
            group_id.clone(),
        );
        let device_ids = device_guard
            .as_ref()
            .unwrap()
            .group_store
            .lock()
            .resolve_ids(vec![&group_id])
            .into_iter()
            .collect::<Vec<String>>();

        let op = Operation::UpdateData(data_id, basic_data);

        if self.tx_coordinator.read().check_tx_state() {
            self.tx_coordinator.write().add_op_to_cur_tx(op);
            Ok(())
        } else {
            match self
                .send_message(device_ids, &Operation::to_string(&op).unwrap())
                .await
            {
                Ok(_) => Ok(()),
                Err(err) => Err(Error::SendFailed(err.to_string())),
            }
        }
    }

    // TODO remove_data

    pub async fn share_data(
        &self,
        data_id: String,
        mut names: Vec<&String>,
    ) -> Result<(), Error> {
        // check that all names are contacts
        for name in names.iter() {
            if !self.is_contact(name) {
                return Err(Error::InvalidContactName(name.to_string()));
            }
        }

        // add own linked_name to names
        let linked_name = self.linked_name();
        names.push(&linked_name);

        // will add children to groups via top_level_names if they exist,
        // otherwise idkeys (currently a linked group is created for every
        // device, so names should always be top_level_names unless this
        // changes). this makes it easier to maintain correct group membership
        // when the subset of devices for various top_level_names changes. so
        // names can be used to search for any existing groups that we can add
        // this piece of data to.
        let device_guard = self.device.read();

        // check that data exists
        let mut data_store_guard =
            device_guard.as_ref().unwrap().data_store.read();
        match data_store_guard.get_data(&data_id) {
            None => return Err(Error::NonexistentData(data_id)),
            Some(data_val) => {
                let mut group_store_guard =
                    device_guard.as_ref().unwrap().group_store.lock();
                let names_strings = names
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<String>>();

                // FIXME just creating new group whenever for now
                //let existing_groups = group_store_guard.get_all_groups();
                //for (group_id, group_val) in existing_groups {
                //}

                let idkeys = group_store_guard
                    .resolve_ids(names.clone())
                    .into_iter()
                    .collect::<Vec<String>>();
                let new_group =
                    Group::new_with_children(None, false, names_strings);
                let _ = group_store_guard.set_group(
                    new_group.group_id().to_string(),
                    new_group.clone(),
                );

                core::mem::drop(group_store_guard);

                // add the new group to all devices to share with
                match self
                    .send_message(
                        idkeys.clone(),
                        &Operation::to_string(&Operation::SetGroup(
                            new_group.group_id().to_string(),
                            new_group.clone(),
                        ))
                        .unwrap(),
                    )
                    .await
                {
                    Ok(_) => {
                        // update the group_id of the relevant data and send the
                        // updated data to all members of the newly formed group
                        self.set_data(
                            data_val.data_id().clone(),
                            data_val.data_type().clone(),
                            data_val.data_val().clone(),
                            Some(new_group.group_id().to_string()),
                        )
                        .await
                    }
                    Err(err) => Err(Error::SendFailed(err.to_string())),
                }
            }
        }
    }

    // TODO unshare_data
}

mod tests {
    use crate::client::{NoiseKVClient, Operation};

    #[tokio::test]
    async fn test_send_one_message() {
        let mut client_0 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, false, None, None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        println!("client_0 idkey = {:?}", client_0.idkey());
        println!("client_1 idkey = {:?}", client_1.idkey());

        // send operation
        let operation =
            Operation::to_string(&Operation::Test("hello".to_string()))
                .unwrap();
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
        let mut client_0 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        // send operation 1
        let operation_1 =
            Operation::to_string(&Operation::Test("hello".to_string()))
                .unwrap();
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
            Operation::to_string(&Operation::Test("goodbye".to_string()))
                .unwrap();
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
        let mut client_0 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;

        client_0.create_standalone_device();
        client_1.create_standalone_device();

        // send operation 1
        let operation_1 =
            Operation::to_string(&Operation::Test("hello".to_string()))
                .unwrap();
        println!("sending operation to device 0");
        client_1
            .send_message(vec![client_0.idkey()], &operation_1)
            .await;

        // send operation 2
        let operation_2 =
            Operation::to_string(&Operation::Test("goodbye".to_string()))
                .unwrap();
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
        let mut client_0 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;

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
        let mut client_0 =
            NoiseKVClient::new(None, None, false, Some(0), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_2 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_3 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_4 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_5 =
            NoiseKVClient::new(None, None, false, Some(2), None).await;

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
            Operation::to_string(&Operation::Test("small".to_string()))
                .unwrap();
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

    #[tokio::test]
    async fn test_add_contact() {
        let mut client_0 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;

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
        let mut group_store_guard_0 =
            client_device_guard_0.as_ref().unwrap().group_store.lock();
        let mut linked_group_0 = group_store_guard_0
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
        core::mem::drop(group_store_guard_0);

        let group_store_guard_0 =
            client_device_guard_0.as_ref().unwrap().group_store.lock();
        let device_group_0 =
            group_store_guard_0.get_group(device_id_0).unwrap().clone();

        /* client_1 groups */

        let client_device_guard_1 = client_1.device.read();
        let linked_id_1: String = client_device_guard_1
            .as_ref()
            .unwrap()
            .linked_name
            .read()
            .to_string();
        let mut group_store_guard_1 =
            client_device_guard_1.as_ref().unwrap().group_store.lock();
        let mut linked_group_1 = group_store_guard_1
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
        core::mem::drop(group_store_guard_1);

        let group_store_guard_1 =
            client_device_guard_1.as_ref().unwrap().group_store.lock();
        let device_group_1 =
            group_store_guard_1.get_group(device_id_1).unwrap().clone();

        /* asserts */

        // check that clients have each others linked groups
        assert_eq!(
            group_store_guard_0.get_group(&linked_id_1).unwrap(),
            &linked_group_1
        );
        assert_eq!(
            group_store_guard_1.get_group(&linked_id_0).unwrap(),
            &linked_group_0
        );

        // check that clients have each others device groups
        assert_eq!(
            group_store_guard_0.get_group(&device_id_1).unwrap(),
            &device_group_1
        );
        assert_eq!(
            group_store_guard_1.get_group(&device_id_0).unwrap(),
            &device_group_0
        );
    }

    // TODO test adding multiple contacts

    #[tokio::test]
    async fn test_get_all_contacts() {
        let mut client_0 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, false, Some(1), None).await;

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
        let mut client_0 = NoiseKVClient::new(None, None, false).await;
        // upload otkeys to server
        client_0.core.receive_message().await;
        client_0.create_standalone_device();

        let mut client_1 = NoiseKVClient::new(None, None, false).await;
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
            client_1.device.as_ref().unwrap().group_store.lock()
        );
        assert_eq!(client_1.device.as_ref().unwrap().linked_devices().len(), 2);
        client_1.receive_operation().await;
        println!(
            "client_1.device: {:#?}",
            client_1.device.as_ref().unwrap().group_store.lock()
        );
        assert_eq!(client_1.device.as_ref().unwrap().linked_devices().len(), 1);
    }

    #[tokio::test]
    async fn test_delete_other_device() {
        let mut client_0 = NoiseKVClient::new(None, None, false).await;
        // upload otkeys to server
        client_0.core.receive_message().await;
        client_0.create_standalone_device();

        let mut client_1 = NoiseKVClient::new(None, None, false).await;
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
            client_0.device.read().as_ref().unwrap().group_store.lock()
        );
        assert_eq!(client_0.device.read().as_ref().unwrap().linked_devices().len(), 2);
        client_0.delete_other_device(client_1.idkey().clone()).await;
        println!(
            "client_0.device: {:#?}",
            client_0.device.read().as_ref().unwrap().group_store.lock()
        );
        assert_eq!(client_0.device.read().as_ref().unwrap().linked_devices().len(), 1);

        // receive delete operation
        client_1.receive_operation().await;
        assert_eq!(client_1.device.read(), None);
    }

    #[tokio::test]
    async fn test_delete_all_devices() {
        let mut client_0 = NoiseKVClient::new(None, None, false).await;
        // upload otkeys to server
        client_0.core.receive_message().await;
        client_0.create_standalone_device();

        let mut client_1 = NoiseKVClient::new(None, None, false).await;
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
    }
    */
}
