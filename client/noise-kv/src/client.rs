use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

use noise_core::core::{Core, CoreClient};

use crate::data::BasicData;
use crate::devices::Device;
use crate::groups::Group; //, GroupStore};

const BUFFER_SIZE: usize = 20;

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
enum Error {
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
    pub device: Option<Device>,
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
            device: None,
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
        &mut self,
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
        //        .as_ref()
        //        .unwrap()
        //        .data_store()
        //        .validator()
        //        .validate(&data_id, &data_val)
        //  },
        //  _ => true,
        //}
    }

    async fn demux(
        &mut self,
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
                .as_mut()
                .unwrap()
                .confirm_update_linked_group(new_linked_name, new_groups)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::SetGroup(group_id, group_val) => {
                self.device
                    .as_mut()
                    .unwrap()
                    .group_store
                    .set_group(group_id, group_val);
                Ok(())
            }
            Operation::LinkGroups(parent_id, child_id) => self
                .device
                .as_mut()
                .unwrap()
                .group_store
                .link_groups(&parent_id, &child_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::DeleteGroup(group_id) => {
                self.device
                    .as_mut()
                    .unwrap()
                    .group_store
                    .delete_group(&group_id);
                Ok(())
            }
            Operation::AddParent(group_id, parent_id) => self
                .device
                .as_mut()
                .unwrap()
                .group_store
                .add_parent(&group_id, &parent_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::RemoveParent(group_id, parent_id) => self
                .device
                .as_mut()
                .unwrap()
                .group_store
                .remove_parent(&group_id, &parent_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::AddChild(group_id, child_id) => self
                .device
                .as_mut()
                .unwrap()
                .group_store
                .add_child(&group_id, &child_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::RemoveChild(group_id, child_id) => self
                .device
                .as_mut()
                .unwrap()
                .group_store
                .remove_child(&group_id, &child_id)
                .map_err(Error::from)
                .map_err(Box::<dyn std::error::Error>::from),
            Operation::UpdateData(data_id, data_val) => {
                self.device
                    .as_mut()
                    .unwrap()
                    .data_store_mut()
                    .set_data(data_id, data_val);
                Ok(())
            }
            Operation::DeleteData(data_id) => {
                self.device
                    .as_mut()
                    .unwrap()
                    .data_store_mut()
                    .delete_data(&data_id);
                Ok(())
            }
            Operation::DeleteSelfDevice => {
                let idkey = self.idkey().clone();
                self.device
                    .as_mut()
                    .unwrap()
                    .delete_device(idkey)
                    .map(|_| self.device = None)
                    .map_err(Error::from)
                    .map_err(Box::<dyn std::error::Error>::from)
            }
            Operation::DeleteOtherDevice(idkey_to_delete) => self
                .device
                .as_mut()
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

    pub fn create_standalone_device(&mut self) {
        self.device = Some(Device::new(self.idkey(), None, None));
    }

    pub async fn create_linked_device(&mut self, idkey: String) {
        self.device =
            Some(Device::new(self.idkey(), None, Some(idkey.clone())));

        let linked_name = &self.device.as_ref().unwrap().linked_name().clone();

        let linked_members_to_add = self
            .device
            .as_mut()
            .unwrap()
            .group_store
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
        &mut self,
        sender: String,
        temp_linked_name: String,
        members_to_add: HashMap<String, Group>,
    ) -> Result<(), Error> {
        self.device
            .as_mut()
            .unwrap()
            .update_linked_group(
                //sender.clone(),
                temp_linked_name.clone(),
                members_to_add,
            )
            .map_err(Error::from);
        let perm_linked_name =
            self.device.as_ref().unwrap().linked_name().to_string();

        // send all groups (TODO and data) to new members
        self.send_message(
            vec![sender],
            &Operation::to_string(&Operation::ConfirmUpdateLinked(
                perm_linked_name,
                self.device
                    .as_ref()
                    .unwrap()
                    .group_store
                    .get_all_groups()
                    .clone(),
            ))
            .unwrap(),
        )
        .await;

        // TODO notify contacts of new members

        Ok(())
    }

    pub async fn delete_self_device(&mut self) -> Result<(), Error> {
        // TODO send to contact devices too
        self.send_message(
            self.device
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
            .as_mut()
            .unwrap()
            .delete_device(idkey)
            .map(|_| self.device = None)
            .map_err(Error::from)
    }

    pub async fn delete_other_device(
        &mut self,
        to_delete: String,
    ) -> Result<(), Error> {
        // TODO send to contact devices too
        self.send_message(
            self.device
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
            .as_mut()
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

    pub async fn delete_all_devices(&mut self) {
        // TODO notify contacts

        // TODO wait for ACK that contacts have indeed received
        // above operations before deleting all devices
        self.send_message(
            self.device
                .as_ref()
                .unwrap()
                .linked_devices()
                .iter()
                .map(|&x| x.clone())
                .collect::<Vec<String>>(),
            &Operation::to_string(&Operation::DeleteSelfDevice).unwrap(),
        )
        .await;
    }
}

/*
mod tests {
    use crate::glue::{Glue, Operation};
    use crate::groups::Group;
    use futures::channel::mpsc;

    #[tokio::test]
    async fn test_channels() {
        let (mut sender, mut receiver) = mpsc::channel::<String>(10);
        let msg = String::from("hello");
        sender.try_send(msg.clone());
        match receiver.try_next() {
            Ok(Some(recv_msg)) => assert_eq!(recv_msg, msg),
            Ok(None) => panic!("None received"),
            Err(err) => panic!("Error: {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_handle_events() {
        let mut glue_0 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_0.core.receive_message().await;
        println!("creating device 0");
        glue_0.create_standalone_device();

        let mut glue_1 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_1.core.receive_message().await;
        println!("creating device 1");
        glue_1.create_standalone_device();

        // send operation
        let operation =
            Operation::to_string(&Operation::Test("hello".to_string())).unwrap();
        println!("sending operation to device 0");
        glue_1.send_message(vec![glue_0.idkey()], &operation).await;

        // receive operation
        println!("getting operation");
        glue_0.receive_operation().await;
    }

    #[tokio::test]
    async fn test_update_linked_group() {
        let mut glue_0 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_0.core.receive_message().await;
        println!("creating device 0");
        glue_0.create_standalone_device();

        let mut glue_1 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_1.core.receive_message().await;
        println!("creating device 1");

        // also sends operation to device 0 to link devices
        glue_1.create_linked_device(glue_0.idkey()).await;

        // receive operation
        println!("getting operation");
        glue_0.receive_operation().await;
    }

    #[tokio::test]
    async fn test_confirm_update_linked_group() {
        let mut glue_0 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_0.core.receive_message().await;

        glue_0.create_standalone_device();

        let mut glue_1 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_1.core.receive_message().await;

        // also sends message to device 0 to link devices
        println!("LINKING <1> to <0>\n");
        glue_1.create_linked_device(glue_0.idkey()).await;
        // receive update_linked...
        println!(
            "Getting update_linked... on <0> and SENDING confirm_update...\n"
        );
        glue_0.receive_operation().await;
        // receive update_linked... loopback
        println!("Getting update_linked... LOOPBACK on <1>\n");
        glue_1.receive_operation().await;
        // receive confirm_update_linked...
        println!("Getting confirm_update... on <1>\n");
        glue_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        println!("Getting confirm_update... LOOPBACK on <0>\n");
        glue_0.receive_operation().await;
    }

    #[tokio::test]
    async fn test_delete_self_device() {
        let mut glue_0 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_0.core.receive_message().await;
        glue_0.create_standalone_device();

        let mut glue_1 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_1.core.receive_message().await;

        // also sends operation to device 0 to link devices
        glue_1.create_linked_device(glue_0.idkey()).await;
        // receive update_linked...
        glue_0.receive_operation().await;
        // receive update_linked... loopback
        glue_1.receive_operation().await;
        // receive confirm_update_linked...
        glue_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        glue_0.receive_operation().await;

        // delete device
        glue_0.delete_self_device().await;
        assert_eq!(glue_0.device, None);

        // receive delete message
        println!(
            "glue_1.device: {:#?}",
            glue_1.device.as_ref().unwrap().group_store
        );
        assert_eq!(glue_1.device.as_ref().unwrap().linked_devices().len(), 2);
        glue_1.receive_operation().await;
        println!(
            "glue_1.device: {:#?}",
            glue_1.device.as_ref().unwrap().group_store
        );
        assert_eq!(glue_1.device.as_ref().unwrap().linked_devices().len(), 1);
    }

    #[tokio::test]
    async fn test_delete_other_device() {
        let mut glue_0 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_0.core.receive_message().await;
        glue_0.create_standalone_device();

        let mut glue_1 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_1.core.receive_message().await;

        // also sends operation to device 0 to link devices
        glue_1.create_linked_device(glue_0.idkey()).await;
        // receive update_linked...
        glue_0.receive_operation().await;
        // receive update_linked... loopback
        glue_1.receive_operation().await;
        // receive confirm_update_linked...
        glue_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        glue_0.receive_operation().await;

        // delete device
        println!(
            "glue_0.device: {:#?}",
            glue_0.device.as_ref().unwrap().group_store
        );
        assert_eq!(glue_0.device.as_ref().unwrap().linked_devices().len(), 2);
        glue_0.delete_other_device(glue_1.idkey().clone()).await;
        println!(
            "glue_0.device: {:#?}",
            glue_0.device.as_ref().unwrap().group_store
        );
        assert_eq!(glue_0.device.as_ref().unwrap().linked_devices().len(), 1);

        // receive delete operation
        glue_1.receive_operation().await;
        assert_eq!(glue_1.device, None);
    }

    #[tokio::test]
    async fn test_delete_all_devices() {
        let mut glue_0 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_0.core.receive_message().await;
        glue_0.create_standalone_device();

        let mut glue_1 = Glue::new(None, None, false);
        // upload otkeys to server
        glue_1.core.receive_message().await;

        // also sends operation to device 0 to link devices
        glue_1.create_linked_device(glue_0.idkey()).await;
        // receive update_linked...
        glue_0.receive_operation().await;
        // receive update_linked... loopback
        glue_1.receive_operation().await;
        // receive confirm_update_linked...
        glue_1.receive_operation().await;
        // receive confirm_update_linked... loopback
        glue_0.receive_operation().await;

        // delete all devices
        glue_0.delete_all_devices().await;
        assert_ne!(glue_0.device, None);
        assert_ne!(glue_1.device, None);

        glue_0.receive_operation().await;
        glue_1.receive_operation().await;
        assert_eq!(glue_0.device, None);
        assert_eq!(glue_1.device, None);
    }
}
*/
