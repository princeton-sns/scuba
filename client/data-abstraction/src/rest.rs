use noise_core::core::{Core, FullPayload};
use serde::{Serialize, Deserialize};
use crate::groups::{Group, Groups};

#[derive(Debug, Serialize, Deserialize)]
enum Message {
//  RequestUpdateLinked,
//  ConfirmUpdateLinked,
//  RequestContact,
//  ConfirmContact,
  LinkGroups(String, String),
  AddParent(String, String),
  AddChild(String, String),
//  AddPermission,
  RemoveParent(String, String),
  RemoveChild(String, String),
//  RemovePermission,
  UpdateGroup(String, Group),
//  UpdateData,
//  DeleteGroup,
//  DeleteData,
//  DeleteDevice,
}

impl Message {
  fn to_string(msg: &Message) -> String {
    serde_json::to_string(msg).unwrap()
  }

  fn from_string(msg: String) -> Message {
    serde_json::from_str(msg.as_str()).unwrap()
  }
}

#[derive(Debug, PartialEq)]
enum Error {
  UnknownMessageType,
  InsufficientPermissionsForAction,
  InsufficientPermissionsForDataMod,
  InsufficientPermissionsForGroupMod,
  DataInvariantViolated,
  SelfIsInvalidContact,
  GroupModErr(crate::groups::Error),
}

pub struct Rest {
  core: Core,
  groups: Groups,
}

impl Rest {
  pub fn new<'a>(
      ip_arg: Option<&'a str>,
      port_arg: Option<&'a str>,
      turn_encryption_off_arg: bool,
  ) -> Rest {
    Self {
      core: Core::new(ip_arg, port_arg, turn_encryption_off_arg),
      groups: Groups::new(),
    }
  }

  async fn send_message(
      &mut self,
      dst_idkeys: Vec<String>,
      payload: &String,
  ) {
    self.core.send_message(dst_idkeys, payload);
  }

  async fn on_message(
      &mut self,
      sender: &String,
      payload: String,
  ) -> Result<(), Error> {
    let message: Message = Message::from_string(payload);
    match self.check_permissions(sender, &message) {
      Ok(_) => {
        // TODO validate data invariants

        // call the demultiplexed function
        self.demux(sender, message) 
      },
      Err(err) => Err(err),
    }
  }

  fn check_permissions(
      &self,
      sender: &String,
      message: &Message,
  ) -> Result<(), Error> {
    // TODO actually check permissions
    match message {
      Message::UpdateGroup(group_id, group_val) => {
        Ok(())
      },
      // TODO are add/remove parent/child ever used outside the context
      // of linking groups?
      Message::AddParent(group_id, parent_id) => {
        Ok(())
      },
      Message::AddChild(group_id, child_id) => {
        Ok(())
      },
      Message::RemoveParent(group_id, parent_id) => {
        Ok(())
      },
      Message::RemoveChild(group_id, child_id) => {
        Ok(())
      },
      Message::LinkGroups(parent_id, child_id) => {
        Ok(())
      },
      _ => Err(Error::UnknownMessageType),
    }
  }

  fn validate_data_invariants() {}

  fn demux(
      &mut self,
      sender: &String,
      message: Message,
  ) -> Result<(), Error> {
    // FIXME not currently in check_permissions() b/c want to call 
    // validate_data_invariants() in-between
    match message {
      Message::UpdateGroup(group_id, group_val) => {
        self.update_group_locally(group_id, group_val)
      },
      Message::AddParent(group_id, parent_id) => {
        self.add_parent_locally(group_id, parent_id)
      },
      Message::AddChild(group_id, child_id) => {
        self.add_child_locally(group_id, child_id)
      },
      Message::RemoveParent(group_id, parent_id) => {
        self.remove_parent_locally(group_id, parent_id)
      },
      Message::RemoveChild(group_id, child_id) => {
        self.remove_child_locally(group_id, child_id)
      },
      Message::LinkGroups(parent_id, child_id) => {
        self.link_groups_locally(parent_id, child_id)
      },
      _ => Err(Error::UnknownMessageType),
    }
  }

  fn update_group_locally(
      &mut self,
      group_id: String,
      group_val: Group,
  ) -> Result<(), Error> {
    self.groups.set_group(group_id, group_val);
    Ok(())
  }

  fn add_parent_locally(
      &mut self,
      group_id: String,
      parent_id: String,
  ) -> Result<(), Error> {
    match self.groups.add_parent(&group_id, &parent_id) {
      Ok(()) => Ok(()),
      Err(err) => Err(Error::GroupModErr(err)),
    }
  }

  fn add_child_locally(
      &mut self,
      group_id: String,
      child_id: String,
  ) -> Result<(), Error> {
    match self.groups.add_child(&group_id, &child_id) {
      Ok(()) => Ok(()),
      Err(err) => Err(Error::GroupModErr(err)),
    }
  }

  fn remove_parent_locally(
      &mut self,
      group_id: String,
      parent_id: String,
  ) -> Result<(), Error> {
    match self.groups.remove_parent(&group_id, &parent_id) {
      Ok(()) => Ok(()),
      Err(err) => Err(Error::GroupModErr(err)),
    }
  }

  fn remove_child_locally(
      &mut self,
      group_id: String,
      child_id: String,
  ) -> Result<(), Error> {
    match self.groups.remove_child(&group_id, &child_id) {
      Ok(()) => Ok(()),
      Err(err) => Err(Error::GroupModErr(err)),
    }
  }

  fn link_groups_locally(
      &mut self,
      parent_id: String,
      child_id: String,
  ) -> Result<(), Error> {
    match self.groups.link_groups(&parent_id, &child_id) {
      Ok(()) => Ok(()),
      Err(err) => Err(Error::GroupModErr(err)),
    }
  }
}

mod tests {
  use crate::rest::{Rest, Message};
  use crate::groups::{Group};

  #[tokio::test]
  async fn test_on_message() {
    let mut rest = Rest::new(None, None, false);

    let group_0 = Group::new(None, None, true, false);
    let group_1 = Group::new(None, None, true, true);

    let dummy_sender = String::from("0");

    let update_group_0_msg = Message::UpdateGroup(
        group_0.group_id().to_string(),
        group_0.clone()
    );

    assert_eq!(Ok(()), rest.on_message(
        &dummy_sender,
        Message::to_string(&update_group_0_msg)
    ).await);
    assert_eq!(
        rest.groups.get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );

    let update_group_1_msg = Message::UpdateGroup(
        group_1.group_id().to_string(),
        group_1.clone()
    );

    assert_eq!(Ok(()), rest.on_message(
        &dummy_sender,
        Message::to_string(&update_group_1_msg)
    ).await);
    assert_eq!(
        rest.groups.get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );

    let add_parent_msg = Message::AddParent(
        group_0.group_id().to_string(),
        group_1.group_id().to_string()
    );

    assert_eq!(Ok(()), rest.on_message(
        &dummy_sender,
        Message::to_string(&add_parent_msg)
    ).await);
    assert_ne!(
        rest.groups.get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );
    assert_eq!(
        rest.groups.get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );

    let remove_parent_msg = Message::RemoveParent(
        group_0.group_id().to_string(),
        group_1.group_id().to_string()
    );

    assert_eq!(Ok(()), rest.on_message(
        &dummy_sender,
        Message::to_string(&remove_parent_msg)
    ).await);
    assert_eq!(
        rest.groups.get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );
    assert_eq!(
        rest.groups.get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );
  }
}

