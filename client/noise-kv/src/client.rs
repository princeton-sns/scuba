use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use parking_lot::RwLock;

use noise_core::core::{Core, CoreClient};

use crate::data::BasicData;
use crate::devices::Device;
use crate::groups::Group;

#[derive(Debug, Serialize, Deserialize, Clone)]
enum Operation {
    UpdateLinked(String, String, HashMap<String, Group>),
    // TODO last param (for data): HashMap<String, BasicData>
    ConfirmUpdateLinked(String, HashMap<String, Group>),
    //  UpdateContact,
    //  ConfirmUpdatedContact,
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
}

impl Operation {
    fn to_string(msg: &Operation) -> Result<String, serde_json::Error> {
        serde_json::to_string(msg)
    }

    fn from_string(msg: String) -> Result<Operation, serde_json::Error> {
        serde_json::from_str(msg.as_str())
    }
}

#[derive(Debug, PartialEq, Error)]
pub enum Error {
    #[error("")]
    InsufficientPermissions,
    #[error("")]
    DataInvariantViolated,
    #[error("")]
    SelfIsInvalidContact,
    #[error("")]
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
}

#[derive(Clone)]
pub struct NoiseKVClient {
    core: Option<Arc<Core<NoiseKVClient>>>,
    pub device: Arc<RwLock<Option<Device>>>,
}

#[async_trait]
impl CoreClient for NoiseKVClient {
    // TODO change interface to return result
    async fn client_callback(
        &self,
        sender: String,
        message: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("IN NOISEKV CLIENT CALLBACK");
        match Operation::from_string(message.clone()) {
            Ok(operation) => {
                match self.check_permissions(&sender, &operation) {
                    Ok(_) => {
                        if self.validate_data_invariants(&operation) {
                            return self.demux(operation).await;
                        }
                        Err(Box::new(Error::DataInvariantViolated))
                    }
                    Err(err) => Err(Box::new(err)),
                }
            }
            Err(_) => Err(Box::new(Error::StringConversionErr(message))),
        }
    }
}

impl NoiseKVClient {
    pub async fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        turn_encryption_off_arg: bool,
    ) -> NoiseKVClient {
        let mut client = NoiseKVClient {
            core: None,
            device: Arc::new(RwLock::new(None)),
        };
        let core = Core::new(
            ip_arg,
            port_arg,
            turn_encryption_off_arg,
            Some(Arc::new(client.clone())),
        )
        .await;

        client.core = Some(core);
        client
    }

    pub fn idkey(&self) -> String {
        self.core.as_ref().unwrap().idkey()
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
        // new_groups) => {        Ok(())
        //    }
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

    // FIXME also call validate() on DeleteData operations
    fn validate_data_invariants(&self, _operation: &Operation) -> bool {
        true
        //match operation {
        //  Operation::UpdateData(data_id, data_val) => {
        //  //| Operation::DeleteData =>
        //    self.device
        //        .read()
        //        .as_ref()
        //        .unwrap()
        //        .data_store
        //        .read()
        //        .validator()
        //        .validate(&data_id, &data_val)
        //  },
        //  _ => true,
        //}
    }

    async fn demux(
        &self,
        //_sender: &String,
        operation: Operation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match operation {
            Operation::UpdateLinked(
                sender,
                temp_linked_name,
                members_to_add,
            ) => self
                .update_linked_group(sender, temp_linked_name, members_to_add)
                .await
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::ConfirmUpdateLinked(new_linked_name, new_groups) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .confirm_update_linked_group(new_linked_name, new_groups)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
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
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
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
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::RemoveParent(group_id, parent_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .remove_parent(&group_id, &parent_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::AddChild(group_id, child_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .add_child(&group_id, &child_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::RemoveChild(group_id, child_id) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .group_store
                .lock()
                .remove_child(&group_id, &child_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
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
                    .map_err(Box::<dyn std::error::Error>::from)
            }
            Operation::DeleteOtherDevice(idkey_to_delete) => self
                .device
                .read()
                .as_ref()
                .unwrap()
                .delete_device(idkey_to_delete)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::Test(msg) => {
                println!("msg: {:?}", msg);
                Ok(())
            }
        }
    }

    /* Remaining functionality */

    pub fn create_standalone_device(&self) {
        *self.device.write() = Some(Device::new(self.idkey(), None, None));
    }

    pub async fn create_linked_device(&self, idkey: String) {
        *self.device.write() =
            Some(Device::new(self.idkey(), None, Some(idkey.clone())));

        let linked_name = &self.device.read().as_ref().unwrap().linked_name.read().clone();

        let linked_members_to_add = self
            .device
            .read()
            .as_ref()
            .unwrap()
            .group_store
            .lock()
            .get_all_subgroups(linked_name);

        self.send_message(
            vec![idkey],
            &Operation::to_string(&Operation::UpdateLinked(
                self.idkey(),
                linked_name.to_string(),
                linked_members_to_add,
            ))
            .unwrap(),
        )
        .await;
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
            .update_linked_group(
                //sender.clone(),
                temp_linked_name.clone(),
                members_to_add,
            )
            .map_err(Error::from);
        let perm_linked_name =
            self.device.read().as_ref().unwrap().linked_name.read().to_string();

        // send all groups (TODO and data) to new members
        self.send_message(
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
            ))
            .unwrap(),
        )
        .await;

        // TODO notify contacts of new members

        Ok(())
    }

    pub async fn delete_self_device(&self) -> Result<(), Error> {
        // TODO send to contact devices too
        self.send_message(
            self.device
                .read()
                .as_ref()
                .unwrap()
                .linked_devices_excluding_self(),
            &Operation::to_string(&Operation::DeleteOtherDevice(self.idkey()))
                .unwrap(),
        )
        .await;

        // TODO wait for ACK that other devices have indeed received
        // above operations before deleting current device
        let idkey = self.idkey().clone();
        self.device
            .read()
            .as_ref()
            .unwrap()
            .delete_device(idkey)
            .map(|_| *self.device.write() = None)
            .map_err(Error::from)
    }

    pub async fn delete_other_device(
        &self,
        to_delete: String,
    ) -> Result<(), Error> {
        // TODO send to contact devices too
        self.send_message(
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
        .await;

        self.device
            .read()
            .as_ref()
            .unwrap()
            .delete_device(to_delete.clone())
            .map_err(Error::from);

        // TODO wait for ACK that other devices have indeed received
        // above operations before deleting specified device
        self.send_message(
            vec![to_delete.clone()],
            &Operation::to_string(&Operation::DeleteSelfDevice).unwrap(),
        )
        .await;

        Ok(())
    }

    pub async fn delete_all_devices(&self) {
        // TODO notify contacts

        // TODO wait for ACK that contacts have indeed received
        // above operations before deleting all devices
        self.send_message(
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
        .await;
    }
}

mod tests {
    use crate::client::{NoiseKVClient, Operation};

    #[tokio::test]
    async fn test_handle_events() {
        let mut client_0 = NoiseKVClient::new(None, None, false).await;
        println!("creating device 0");
        client_0.create_standalone_device();

        let mut client_1 = NoiseKVClient::new(None, None, false).await;
        println!("creating device 1");
        client_1.create_standalone_device();

        // send operation
        let operation =
            Operation::to_string(&Operation::Test("hello".to_string())).unwrap();
        println!("sending operation to device 0");
        client_1.send_message(vec![client_0.idkey()], &operation).await;

        // FIXME exits before client_callback can run - how to wait?
    }

    #[tokio::test]
    async fn test_update_linked_group() {
        let mut client_0 = NoiseKVClient::new(None, None, false).await;
        println!("creating device 0");
        client_0.create_standalone_device();

        let mut client_1 = NoiseKVClient::new(None, None, false).await;

        println!("creating device 1");
        // also sends operation to device 0 to link devices
        client_1.create_linked_device(client_0.idkey()).await;

        // FIXME exits before client_callback can run - how to wait?
    }

/*
    #[tokio::test]
    async fn test_confirm_update_linked_group() {
        let mut client_0 = NoiseKVClient::new(None, None, false).await;
        // upload otkeys to server
        client_0.core.receive_message().await;

        client_0.create_standalone_device();

        let mut client_1 = NoiseKVClient::new(None, None, false).await;
        // upload otkeys to server
        client_1.core.receive_message().await;

        // also sends message to device 0 to link devices
        println!("LINKING <1> to <0>\n");
        client_1.create_linked_device(client_0.idkey()).await;
        // receive update_linked...
        println!(
            "Getting update_linked... on <0> and SENDING confirm_update...\n"
        );
        client_0.receive_operation().await;
        // receive update_linked... loopback
        println!("Getting update_linked... LOOPBACK on <1>\n");
        client_1.receive_operation().await;
        // receive confirm_update_linked...
        println!("Getting confirm_update... on <1>\n");
        client_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        println!("Getting confirm_update... LOOPBACK on <0>\n");
        client_0.receive_operation().await;
    }

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
