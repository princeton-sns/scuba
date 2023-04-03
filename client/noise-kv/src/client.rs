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
use crate::metadata::{Group, PermType, PermissionSet};

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
    SetPerm(String, PermissionSet),
    AddPermMembers(String, Option<String>, PermType),
    // TODO RemovePermMember
    SetGroup(String, Group),
    SetGroups(HashMap<String, Group>),
    //LinkGroups(String, String),
    //DeleteGroup(String),
    //AddParent(String, String),
    //RemoveParent(String, String),
    //AddChild(String, String),
    //RemoveChild(String, String),
    UpdateData(String, BasicData),
    DeleteData(String),
    DeleteSelfDevice,
    DeleteOtherDevice(String),
    Test(String),
}

impl Operation {
    fn to_string(msg: &Operation) -> Result<String, serde_json::Error> {
        serde_json::to_string(msg)
    }

    fn from_string(msg: String) -> Result<Operation, serde_json::Error> {
        serde_json::from_str(msg.as_str())
    }
}

#[derive(Clone)]
pub struct NoiseKVClient {
    core: Option<Arc<Core<NoiseKVClient>>>,
    pub device: Arc<RwLock<Option<Device<BasicData>>>>,
    ctr: Arc<Mutex<u32>>,
    ctr_cv: Arc<Condvar>,
    sec_wait_to_apply: Arc<Option<u64>>,
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
                println!("");
                println!("OPERATION: {:?}", operation);
                match self.check_permissions(&sender, &operation) {
                    Ok(_) => {
                        if self.validate_data_invariants(&operation) {
                            match self.demux(operation).await {
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

        let mut ctr = self.ctr.lock();
        if *ctr != 0 {
            //println!("cb_ctr: {:?}", *ctr);
            //println!("");
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

    /* Sending-side functions */

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

    /* Receiving-side functions */

    // TODO put each permission check in own function if will be used by
    // sending-side code too (which can then just call the respective func)
    fn check_permissions(
        &self,
        sender: &String,
        operation: &Operation,
    ) -> Result<(), Error> {
        match operation {
            /* Dummy op */
            Operation::Test(msg) => Ok(()),
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
            Operation::ConfirmUpdateLinked(
                new_linked_name,
                new_groups,
                new_data,
            ) => {
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
            Operation::SetPerm(perm_id, perm_val) => {
                // if permissions set with this id already exists, error
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
                    return Err(Error::PermAlreadyExists(perm_id.to_string()));
                }
                Ok(())
            }
            Operation::AddPermMembers(perm_id, group_id_opt, new_members) => {
                if !self
                    .device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .has_metadata_mod_permissions(sender, perm_id)
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
                //println!("group_val: {:?}", group_val);
                //let mut cur_group_val = group_val;
                //while cur_group_val.perm_id().is_none() {
                //    // multiple parents...
                //    cur_group_val = self
                //        .device
                //        .read()
                //        .as_ref()
                //        .unwrap()
                //        .meta_store
                //        .read()
                //        .get_group
                //}
                Ok(())
            }
            Operation::SetGroups(groups) => Ok(()),
            //Operation::LinkGroups(parent_id, child_id) => Ok(()),
            //Operation::DeleteGroup(group_id) => Ok(()),
            //Operation::AddParent(group_id, parent_id) => Ok(()),
            //Operation::RemoveParent(group_id, parent_id) => Ok(()),
            //Operation::AddChild(group_id, child_id) => Ok(()),
            /* Need data-mod permissions */
            Operation::UpdateData(data_id, data_val) => {
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
                            .has_data_mod_permissions(
                                sender,
                                data_val.perm_id(),
                            )
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
            Operation::AddPermMembers(perm_id, group_id_opt, new_members) => {
                self.device
                    .read()
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .write()
                    .add_permissions(&perm_id, group_id_opt, new_members)
                    .map_err(Error::from)
            }
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
            //Operation::AddParent(group_id, parent_id) => self
            //    .device
            //    .read()
            //    .as_ref()
            //    .unwrap()
            //    .meta_store
            //    .write()
            //    .add_parent(&group_id, &parent_id)
            //    .map_err(Error::from),
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
            .meta_store
            .read()
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

    // FIXME breaks once share data; should probably be either app-level
    // or impl differently
    pub fn get_contacts(&self) -> HashSet<String> {
        self.device.read().as_ref().unwrap().get_contacts()
    }

    fn is_contact(&self, name: &String) -> bool {
        match self
            .device
            .read()
            .as_ref()
            .unwrap()
            .meta_store
            .read()
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
            .meta_store
            .read()
            .is_group_member(&contact_idkey, &linked_name)
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
            .meta_store
            .read()
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

    fn get_group_ids_from_perm_val(
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

    //fn get_group_ids_from_perm_id(&self, perm_id: &String) -> Vec<String> {
    //    match self
    //        .device
    //        .read()
    //        .as_ref()
    //        .unwrap()
    //        .meta_store
    //        .read()
    //        .get_perm(perm_id)
    //    {
    //        Some(perm_val) => {
    //            self.get_group_ids_from_perm_val(perm_val)
    //        }
    //        None => { Vec::<String>::new() }
    //    }
    //}

    pub async fn set_data(
        &self,
        data_id: String,
        data_type: String,
        data_val: String,
        data_reader_idkeys: Option<Vec<String>>,
    ) -> Result<(), Error> {
        // FIXME check write permissions

        let device_guard = self.device.read();
        let data_store_guard = device_guard.as_ref().unwrap().data_store.read();
        let existing_val = data_store_guard.get_data(&data_id).clone();

        // if data exists, use existing perms; otherwise create new one
        let perm_id;
        let mut perm_val;
        let device_ids: Vec<String>;
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
                    let group_ids = self.get_group_ids_from_perm_val(&perm_val);
                    device_ids = device_guard
                        .as_ref()
                        .unwrap()
                        .meta_store
                        .read()
                        .resolve_group_ids(
                            group_ids.iter().collect::<Vec<&String>>(),
                        )
                        .into_iter()
                        .collect::<Vec<String>>();
                } else {
                    device_ids = data_reader_idkeys.unwrap();
                }
            }
            None => {
                // create new perms for this data_val
                perm_val = PermissionSet::new(None, None, None, None);
                perm_id = perm_val.perm_id().to_string();

                // create owner group that includes self.linked_name()
                let group_val = Group::new(
                    None,
                    Some(perm_id.to_string()),
                    false,
                    Some(Some(vec![self.linked_name().to_string()])),
                );

                // add owner group into perms
                perm_val.set_owners(group_val.group_id());

                println!("perm_val: {:?}", perm_val.clone());

                let idkeys = device_guard
                    .as_ref()
                    .unwrap()
                    .meta_store
                    .read()
                    .resolve_group_ids(vec![&self.linked_name()])
                    .into_iter()
                    .collect::<Vec<String>>();

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
                    .send_message(
                        idkeys.clone(),
                        &Operation::to_string(&Operation::SetPerm(
                            perm_val.perm_id().to_string(),
                            perm_val.clone(),
                        ))
                        .unwrap(),
                    )
                    .await;

                if res.is_err() {
                    return Err(Error::SendFailed(
                        res.err().unwrap().to_string(),
                    ));
                }

                // send group
                res = self
                    .send_message(
                        idkeys.clone(),
                        &Operation::to_string(&Operation::SetGroup(
                            group_val.group_id().to_string(),
                            group_val.clone(),
                        ))
                        .unwrap(),
                    )
                    .await;

                if res.is_err() {
                    return Err(Error::SendFailed(
                        res.err().unwrap().to_string(),
                    ));
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

        match self
            .send_message(
                // includes idkeys of _all_ permissions
                // (including data-only readers)
                device_ids.clone(),
                &Operation::to_string(&Operation::UpdateData(
                    data_id, basic_data,
                ))
                .unwrap(),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::SendFailed(err.to_string())),
        }
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

    //pub async fn add_readers(
    //    &self,
    //    data_id: String,
    //    mut names: Vec<&String>,
    //) -> Result<(), Error> {
    //}

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
        mut new_members_refs: Vec<&String>, // FIXME one or the other
    ) -> Result<(), Error> {
        let device_guard = self.device.read();
        let mut data_store_guard =
            device_guard.as_ref().unwrap().data_store.read();

        // check that data exists
        match data_store_guard.get_data(&data_id) {
            None => return Err(Error::NonexistentData(data_id)),
            Some(data_val) => {
                let mut meta_store_guard =
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
                // existing owners
                match perm_val.owners() {
                    Some(owners_group_id) => {
                        metadata_readers.push(owners_group_id);
                    }
                    None => {}
                }
                // existing writers
                match perm_val.writers() {
                    Some(writers_group_id) => {
                        metadata_readers.push(writers_group_id);
                    }
                    None => {}
                }
                // existing readers
                match perm_val.readers() {
                    Some(readers_group_id) => {
                        metadata_readers.push(readers_group_id);
                    }
                    None => {}
                }
                // PER ADDED PERM new member
                match new_members {
                    PermType::Owners(_)
                    | PermType::Writers(_)
                    | PermType::Readers(_) => {
                        metadata_readers.append(&mut new_members_refs.clone())
                    } // skip append if PermType::DataOnlyReaders
                }

                // resolve the above to device ids
                let metadata_reader_idkeys = meta_store_guard
                    .resolve_group_ids(metadata_readers)
                    .into_iter()
                    .collect::<Vec<String>>();

                // TODO add all data-only reader idkeys to send data to
                // if PermType::DataOnlyReaders is implemented
                let data_reader_idkeys = metadata_reader_idkeys
                    .clone()
                    .iter()
                    .map(|idkey| idkey.to_string())
                    .collect::<Vec<String>>();

                /*
                 * Then collect existing perm-associated groups.
                 */

                let mut assoc_groups = HashMap::<String, Group>::new();
                // get owner subgroups
                match perm_val.owners() {
                    Some(owners_group_id) => {
                        assoc_groups.extend(
                            meta_store_guard.get_all_subgroups(owners_group_id),
                        );
                    }
                    None => {}
                }
                // get writer subgroups
                match perm_val.writers() {
                    Some(writers_group_id) => {
                        assoc_groups.extend(
                            meta_store_guard
                                .get_all_subgroups(writers_group_id),
                        );
                    }
                    None => {}
                }
                // get reader subgroups
                match perm_val.readers() {
                    Some(readers_group_id) => {
                        assoc_groups.extend(
                            meta_store_guard
                                .get_all_subgroups(readers_group_id),
                        );
                    }
                    None => {}
                }
                // PER ADDED PERM get new-member subgroups
                for new_member_ref in &new_members_refs {
                    assoc_groups.extend(
                        meta_store_guard.get_all_subgroups(new_member_ref),
                    );
                }
                // PER ADDED PERM create new group if doesn't exist
                match group_id_opt.clone() {
                    Some(new_group_id) => match new_members.clone() {
                        PermType::Owners(new_members_vec)
                        | PermType::Writers(new_members_vec)
                        | PermType::Readers(new_members_vec) => {
                            let new_group = Group::new(
                                Some(new_group_id.clone()),
                                Some(perm_val.perm_id().to_string()),
                                false,
                                Some(Some(new_members_vec.clone())),
                            );
                            assoc_groups.insert(new_group_id, new_group);
                        }
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
                let mut res = self
                    .send_message(
                        metadata_reader_idkeys.clone(),
                        &Operation::to_string(&Operation::SetPerm(
                            perm_val.perm_id().to_string(),
                            perm_val.clone(),
                        ))
                        .unwrap(),
                    )
                    .await;

                if res.is_err() {
                    return Err(Error::SendFailed(
                        res.err().unwrap().to_string(),
                    ));
                }

                // then send SetGroups for all associated subgroups
                // FIXME will overwrite is_contact_name fields
                res = self
                    .send_message(
                        metadata_reader_idkeys.clone(),
                        &Operation::to_string(&Operation::SetGroups(
                            assoc_groups,
                        ))
                        .unwrap(),
                    )
                    .await;

                if res.is_err() {
                    return Err(Error::SendFailed(
                        res.err().unwrap().to_string(),
                    ));
                }

                // PER ADDED PERM send AddPermMembers to all metadata-readers
                res = self
                    .send_message(
                        metadata_reader_idkeys.clone(),
                        &Operation::to_string(&Operation::AddPermMembers(
                            perm_val.perm_id().to_string(),
                            group_id_opt,
                            new_members,
                        ))
                        .unwrap(),
                    )
                    .await;

                if res.is_err() {
                    return Err(Error::SendFailed(
                        res.err().unwrap().to_string(),
                    ));
                }

                // finally, send UpdateData (via set_data) to set the
                // associated data val:

                // copy out and drop read lock on data_store b/c UpdateData
                // will need exclusive lock to set data (on receive)

                let data_id = data_val.data_id().clone();
                let data_type = data_val.data_type().clone();
                let data_val_interior = data_val.data_val().clone();

                core::mem::drop(data_store_guard);

                self.set_data(
                    data_id,
                    data_type,
                    data_val_interior,
                    Some(data_reader_idkeys),
                )
                .await
            }
        }
    }

    // TODO unshare_data

    // TODO metadata_gc
}

mod tests {
    use crate::client::{NoiseKVClient, Operation};
    use crate::data::NoiseData;

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

    /*
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
        client_0.add_contact(client_1.idkey()).await;
    }
    */

    #[tokio::test]
    async fn test_add_writers() {
        // FIXME encrypt
        let mut client_0 =
            NoiseKVClient::new(None, None, true, Some(8), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, true, Some(5), None).await;

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

        let data_type = "type".to_string();
        let data_id = crate::metadata::generate_uuid();
        let json_val = r#"{ data: true }"#.to_string();
        res = client_0
            .set_data(
                data_id.clone(),
                data_type.clone(),
                json_val.clone(),
                None,
            )
            .await;
        if res.is_err() {
            panic!("send failed");
        }

        println!("");
        println!("SET DATA");
        println!("");

        loop {
            let ctr = client_0.ctr.lock();
            println!("ctr_0 (test): {:?}", *ctr);
            if *ctr != 4 {
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
        println!("ADDING WRITERS");

        println!("client_0.idkey: {:?}", client_0.idkey());
        println!("client_0.linked_name: {:?}", client_0.linked_name());
        println!("client_1.idkey: {:?}", client_1.idkey());
        println!("client_1.linked_name: {:?}", client_1.linked_name());

        res = client_0
            .add_writers(
                data_val.data_id().clone(),
                vec![&client_1.linked_name()],
            )
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
        // FIXME encrypt
        let mut client_0 =
            NoiseKVClient::new(None, None, true, Some(10), None).await;
        let mut client_1 =
            NoiseKVClient::new(None, None, true, Some(7), None).await;

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
            .set_data(
                data_id.clone(),
                data_type.clone(),
                json_val.clone(),
                None,
            )
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
            .add_readers(
                data_val.data_id().clone(),
                vec![&client_1.linked_name()],
            )
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
