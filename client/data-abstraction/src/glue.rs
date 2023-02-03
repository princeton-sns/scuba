use futures::channel::mpsc;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use noise_core::core::{Core, FullPayload};

use crate::groups::{Group, Groups};
use crate::devices::Device;

const BUFFER_SIZE: usize = 20;

#[derive(Debug, Serialize, Deserialize, Clone)]
enum Message {
  UpdateLinked(String, String, HashMap<String, Group>),
  // TODO last param (for data): HashMap<String, Data>
  ConfirmUpdateLinked(String, HashMap<String, Group>),
//  UpdateContact,
//  ConfirmUpdatedContact,
  SetGroup(String, Group),
  LinkGroups(String, String),
  AddParent(String, String),
  RemoveParent(String, String),
  AddChild(String, String),
  RemoveChild(String, String), // FIXME may never be used
//  AddPermission,
//  RemovePermission,
//  UpdateData,
//  DeleteData,
//  DeleteGroup,
  DeleteSelfDevice,
  DeleteOtherDevice(String),
  Test(String),
}

impl Message {
  fn to_string(msg: &Message) -> Result<String, serde_json::Error> {
    serde_json::to_string(msg)
  }

  fn from_string(msg: String) -> Result<Message, serde_json::Error> {
    serde_json::from_str(msg.as_str())
  }
}

#[derive(Debug, PartialEq, Error)]
enum Error {
  #[error("")]
  InsufficientPermissionsForAction,
  #[error("")]
  InsufficientPermissionsForDataMod,
  #[error("")]
  InsufficientPermissionsForGroupMod,
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
  #[error("no message available")]
  StreamErr,
}

pub struct Glue {
  core: Core,
  device: Option<Device>,
  receiver: mpsc::Receiver<(String, String)>,
}

impl Glue {
  pub fn new<'a>(
      ip_arg: Option<&'a str>,
      port_arg: Option<&'a str>,
      turn_encryption_off_arg: bool,
  ) -> Glue {
    let (sender, receiver) = mpsc::channel::<(String, String)>(BUFFER_SIZE);
    Self {
      core: Core::new(ip_arg, port_arg, turn_encryption_off_arg, sender),
      device: None,
      receiver,
    }
  }

  pub fn idkey(&self) -> String {
    self.core.idkey()
  }

  pub fn device(&self) -> &Option<Device> {
    &self.device
  }

  pub fn device_mut(&mut self) -> &mut Option<Device> {
    &mut self.device
  }

  /* Sending-side functions */

  async fn send_message(
      &mut self,
      dst_idkeys: Vec<String>,
      payload: &String,
  ) -> reqwest::Result<reqwest::Response> {
    self.core.send_message(dst_idkeys, payload).await
  }

  /* Receiving-side functions */

  async fn handle_core_events(
      &mut self,
  ) -> Result<(), Error> {
    // have core process potential incoming message
    self.core.handle_server_events().await;

    match self.receiver.try_next() {
      Ok(Some((sender, payload))) => {
        match Message::from_string(payload.clone()) {
          Ok(message) => {
            println!("GOT MESSAGE: {:?}", message.clone());
            match self.check_permissions(&sender, &message) {
              Ok(_) => {
                self.validate_data_invariants();

                // call the demultiplexed function
                self.demux(&sender, message).await
              },
              Err(err) => Err(err),
            }
          },
          Err(err) => Err(Error::StringConversionErr(payload)),
        }
      },
      Ok(None) => Ok(()),
      Err(err) => Err(Error::StreamErr),
    }
  }

  fn check_permissions(
      &self,
      sender: &String,
      message: &Message,
  ) -> Result<(), Error> {
    // TODO actually check permissions
    match message {
      Message::UpdateLinked(sender, temp_linked_name, members_to_add) => {
        Ok(())
      },
      Message::ConfirmUpdateLinked(new_linked_name, new_groups) => {
        Ok(())
      },
      Message::SetGroup(group_id, group_val) => {
        Ok(())
      },
      Message::LinkGroups(parent_id, child_id) => {
        Ok(())
      },
      Message::AddParent(group_id, parent_id) => {
        Ok(())
      },
      Message::RemoveParent(group_id, parent_id) => {
        Ok(())
      },
      Message::AddChild(group_id, child_id) => {
        Ok(())
      },
      Message::RemoveChild(group_id, child_id) => {
        Ok(())
      },
      Message::DeleteSelfDevice => {
        Ok(())
      },
      Message::DeleteOtherDevice(idkey_to_delete) => {
        Ok(())
      },
      Message::Test(msg) => {
        Ok(())
      },
    }
  }

  fn validate_data_invariants(
      &self
  ) -> Result<(), Error> {
    // TODO actually validate data invariants
    Ok(())
  }

  async fn demux(
      &mut self,
      sender: &String,
      message: Message,
  ) -> Result<(), Error> {
    // not in check_permissions() b/c want to call 
    // validate_data_invariants() in-between
    match message {
      Message::UpdateLinked(sender, temp_linked_name, members_to_add) => {
        self.update_linked_group(sender, temp_linked_name, members_to_add)
            .await
            .map_err(Error::from)
      },
      Message::ConfirmUpdateLinked(new_linked_name, new_groups) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .confirm_update_linked_group(
                new_linked_name,
                new_groups
            )
            .map_err(Error::from)
      },
      Message::SetGroup(group_id, group_val) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .groups_mut()
            .set_group(group_id, group_val);
        Ok(())
      },
      Message::LinkGroups(parent_id, child_id) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .groups_mut()
            .link_groups(&parent_id, &child_id)
            .map_err(Error::from)
      },
      Message::AddParent(group_id, parent_id) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .groups_mut()
            .add_parent(&group_id, &parent_id)
            .map_err(Error::from)
      },
      Message::RemoveParent(group_id, parent_id) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .groups_mut()
            .remove_parent(&group_id, &parent_id)
            .map_err(Error::from)
      },
      Message::AddChild(group_id, child_id) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .groups_mut()
            .add_child(&group_id, &child_id)
            .map_err(Error::from)
      },
      Message::RemoveChild(group_id, child_id) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .groups_mut()
            .remove_child(&group_id, &child_id)
            .map_err(Error::from)
      },
      Message::DeleteSelfDevice => {
        let idkey = self.idkey().clone();
        self.device_mut()
            .as_mut()
            .unwrap()
            .delete_device(idkey)
            .map(|_| self.device = None)
            .map_err(Error::from)
      },
      Message::DeleteOtherDevice(idkey_to_delete) => {
        self.device_mut()
            .as_mut()
            .unwrap()
            .delete_device(idkey_to_delete)
            .map_err(Error::from)
      },
      Message::Test(msg) => {
        println!("msg");
        Ok(())
      },
    }
  }

  /* Remaining functionality */

  pub fn create_standalone_device(&mut self) {
    self.device = Some(Device::new(self.core.idkey(), None, None));
  }

  pub async fn create_linked_device(&mut self, idkey: String) {
    self.device = Some(Device::new(self.core.idkey(), None, Some(idkey.clone())));

    let linked_name = &self.device()
        .as_ref()
        .unwrap()
        .linked_name()
        .clone();

    let linked_members_to_add = self.device_mut()
        .as_mut()
        .unwrap()
        .groups()
        .get_all_subgroups(linked_name);

    self.send_message(
        vec![idkey],
        &Message::to_string(&Message::UpdateLinked(
            self.core.idkey(),
            linked_name.to_string(),
            linked_members_to_add,
        )).unwrap(),
    ).await;
  }

  async fn update_linked_group(
      &mut self,
      sender: String,
      temp_linked_name: String,
      members_to_add: HashMap<String, Group>,
  ) -> Result<(), Error> {
    self.device_mut()
        .as_mut()
        .unwrap()
        .update_linked_group(sender.clone(), temp_linked_name.clone(), members_to_add)
        .map_err(Error::from);
    let perm_linked_name = self.device().as_ref().unwrap().linked_name().to_string();

    // send all groups (TODO and data) to new members
    self.send_message(
        vec![sender],
        &Message::to_string(&Message::ConfirmUpdateLinked(
            perm_linked_name,
            self.device()
                .as_ref()
                .unwrap()
                .groups()
                .get_all_groups()
                .clone()
        )).unwrap(),
    ).await;

    // TODO notify contacts of new members

    Ok(())
  }

  pub async fn delete_self_device(&mut self) -> Result<(), Error> {
    // TODO send to contact devices too
    self.send_message(
        self.device().as_ref().unwrap().linked_devices_excluding_self(),
        &Message::to_string(&Message::DeleteOtherDevice(
            self.idkey()
        )).unwrap()
    ).await;

    // TODO wait for ACK that other devices have indeed received above
    // messages before deleting current device
    let idkey = self.idkey().clone();
    self.device_mut()
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
        self.device()
            .as_ref()
            .unwrap()
            .linked_devices_excluding_self_and_other(&to_delete),
        &Message::to_string(&Message::DeleteOtherDevice(
            to_delete.clone()
        )).unwrap()
    ).await;

    self.device_mut()
        .as_mut()
        .unwrap()
        .delete_device(to_delete.clone())
        .map_err(Error::from);

    // TODO wait for ACK that other devices have indeed received above
    // messages before deleting specified device
    self.send_message(
      vec![to_delete.clone()],
      &Message::to_string(&Message::DeleteSelfDevice).unwrap()
    ).await;

    Ok(())
  }

  pub async fn delete_all_devices(&mut self) {
    // TODO notify contacts

    // TODO wait for ACK that contacts have indeed received above
    // messages before deleting all devices
    self.send_message(
        self.device()
            .as_ref()
            .unwrap()
            .linked_devices()
            .iter()
            .map(|&x| x.clone())
            .collect::<Vec::<String>>(),
        &Message::to_string(&Message::DeleteSelfDevice).unwrap()
    ).await;
  }
}

mod tests {
  use crate::glue::{Glue, Message};
  use crate::groups::{Group};
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
    glue_0.core.handle_server_events().await;
    println!("creating device 0");
    glue_0.create_standalone_device();

    let mut glue_1 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_1.core.handle_server_events().await;
    println!("creating device 1");
    glue_1.create_standalone_device();

    // send message
    let message = Message::to_string(
        &Message::Test("hello".to_string())
    ).unwrap();
    println!("sending message to device 0");
    glue_1.send_message(vec![glue_0.idkey()], &message).await;

    // receive message
    println!("getting message");
    glue_0.handle_core_events().await;
  }

  #[tokio::test]
  async fn test_update_linked_group() {
    let mut glue_0 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_0.core.handle_server_events().await;
    println!("creating device 0");
    glue_0.create_standalone_device();

    let mut glue_1 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_1.core.handle_server_events().await;
    println!("creating device 1");

    // also sends message to device 0 to link devices
    glue_1.create_linked_device(glue_0.idkey()).await;

    // receive message
    println!("getting message");
    glue_0.handle_core_events().await;
  }

  #[tokio::test]
  async fn test_confirm_update_linked_group() {
    let mut glue_0 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_0.core.handle_server_events().await;

    glue_0.create_standalone_device();

    let mut glue_1 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_1.core.handle_server_events().await;

    // also sends message to device 0 to link devices
    println!("LINKING <1> to <0>\n");
    glue_1.create_linked_device(glue_0.idkey()).await;
    // receive update_linked...
    println!("Getting update_linked... on <0> and SENDING confirm_update...\n");
    glue_0.handle_core_events().await;
    // receive update_linked... loopback
    println!("Getting update_linked... LOOPBACK on <1>\n");
    glue_1.handle_core_events().await;
    // receive confirm_update_linked...
    println!("Getting confirm_update... on <1>\n");
    glue_1.handle_core_events().await;
    // receive confirm_update_linked... loopback
    println!("Getting confirm_update... LOOPBACK on <0>\n");
    glue_0.handle_core_events().await;
  }

  #[tokio::test]
  async fn test_delete_self_device() {
    let mut glue_0 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_0.core.handle_server_events().await;
    glue_0.create_standalone_device();

    let mut glue_1 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_1.core.handle_server_events().await;

    // also sends message to device 0 to link devices
    glue_1.create_linked_device(glue_0.idkey()).await;
    // receive update_linked...
    glue_0.handle_core_events().await;
    // receive update_linked... loopback
    glue_1.handle_core_events().await;
    // receive confirm_update_linked...
    glue_1.handle_core_events().await;
    // receive confirm_update_linked... loopback
    glue_0.handle_core_events().await;

    // delete device
    glue_0.delete_self_device().await;
    assert_eq!(glue_0.device(), &None);

    // receive delete message
    println!("glue_1.device: {:#?}", glue_1.device().as_ref().unwrap().groups());
    assert_eq!(glue_1.device().as_ref().unwrap().linked_devices().len(), 2);
    glue_1.handle_core_events().await;
    println!("glue_1.device: {:#?}", glue_1.device().as_ref().unwrap().groups());
    assert_eq!(glue_1.device().as_ref().unwrap().linked_devices().len(), 1);
  }

  #[tokio::test]
  async fn test_delete_other_device() {
    let mut glue_0 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_0.core.handle_server_events().await;
    glue_0.create_standalone_device();

    let mut glue_1 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_1.core.handle_server_events().await;

    // also sends message to device 0 to link devices
    glue_1.create_linked_device(glue_0.idkey()).await;
    // receive update_linked...
    glue_0.handle_core_events().await;
    // receive update_linked... loopback
    glue_1.handle_core_events().await;
    // receive confirm_update_linked...
    glue_1.handle_core_events().await;
    // receive confirm_update_linked... loopback
    glue_0.handle_core_events().await;

    // delete device
    println!("glue_0.device: {:#?}", glue_0.device().as_ref().unwrap().groups());
    assert_eq!(glue_0.device().as_ref().unwrap().linked_devices().len(), 2);
    glue_0.delete_other_device(glue_1.idkey().clone()).await;
    println!("glue_0.device: {:#?}", glue_0.device().as_ref().unwrap().groups());
    assert_eq!(glue_0.device().as_ref().unwrap().linked_devices().len(), 1);

    // receive delete message
    glue_1.handle_core_events().await;
    assert_eq!(glue_1.device(), &None);
  }

  #[tokio::test]
  async fn test_delete_all_devices() {
    let mut glue_0 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_0.core.handle_server_events().await;
    glue_0.create_standalone_device();

    let mut glue_1 = Glue::new(None, None, false);
    // upload otkeys to server
    glue_1.core.handle_server_events().await;

    // also sends message to device 0 to link devices
    glue_1.create_linked_device(glue_0.idkey()).await;
    // receive update_linked...
    glue_0.handle_core_events().await;
    // receive update_linked... loopback
    glue_1.handle_core_events().await;
    // receive confirm_update_linked...
    glue_1.handle_core_events().await;
    // receive confirm_update_linked... loopback
    glue_0.handle_core_events().await;

    // delete all devices
    glue_0.delete_all_devices().await;
    assert_ne!(glue_0.device(), &None);
    assert_ne!(glue_1.device(), &None);

    glue_0.handle_core_events().await;
    glue_1.handle_core_events().await;
    assert_eq!(glue_0.device(), &None);
    assert_eq!(glue_1.device(), &None);
  }

/*
  #[tokio::test]
  async fn test_handle_core_events() {
    let mut glue = Glue::new(None, None, false);

    let group_0 = Group::new(None, None, true, false);
    let group_1 = Group::new(None, None, true, true);

    let dummy_sender = String::from("0");

    let update_group_0_msg = Message::SetGroup(
        group_0.group_id().to_string(),
        group_0.clone()
    );

    assert_eq!(Ok(()), glue.handle_core_events(
        &dummy_sender,
        Message::to_string(&update_group_0_msg).unwrap()
    ).await);
    assert_eq!(
        glue.device.groups().get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );

    let update_group_1_msg = Message::SetGroup(
        group_1.group_id().to_string(),
        group_1.clone()
    );

    assert_eq!(Ok(()), glue.handle_core_events(
        &dummy_sender,
        Message::to_string(&update_group_1_msg).unwrap()
    ).await);
    assert_eq!(
        glue.device.groups().get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );

    let add_parent_msg = Message::AddParent(
        group_0.group_id().to_string(),
        group_1.group_id().to_string()
    );

    assert_eq!(Ok(()), glue.handle_core_events(
        &dummy_sender,
        Message::to_string(&add_parent_msg).unwrap()
    ).await);
    assert_ne!(
        glue.device.groups().get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );
    assert_eq!(
        glue.device.groups().get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );

    let remove_parent_msg = Message::RemoveParent(
        group_0.group_id().to_string(),
        group_1.group_id().to_string()
    );

    assert_eq!(Ok(()), glue.handle_core_events(
        &dummy_sender,
        Message::to_string(&remove_parent_msg).unwrap()
    ).await);
    assert_eq!(
        glue.device.groups().get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );
    assert_eq!(
        glue.device.groups().get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );
  }
*/
}
