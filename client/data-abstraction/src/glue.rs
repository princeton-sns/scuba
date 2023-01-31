use noise_core::core::{Core, FullPayload};
use serde::{Serialize, Deserialize};
use crate::groups::{Group, Groups};
use thiserror::Error;

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

#[derive(Debug, PartialEq, Error)]
enum Error {
  #[error("")]
  UnknownMessageType,
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
  #[error(transparent)]
  GroupErr {
    #[from]
    source: crate::groups::Error,
  },
}

//impl Error {
//  fn to_glue_error(res: Result<(), 
//}

/* Categories of operations ~= potentially separate structs */

// Devices
//
// init_device
// create_independent_device
// create_linked_device
// delete_current_device
// delete_linked_device
// delete_all_devices
// delete_device (locally/remotely)
// get/set/remove pending_link_idkey
// request_update_linked
// process_update_linked_request
// confirm_update_linked
// process_confirm_update_linked
// get_linked_name
// get_linked_devices

// Contacts
//
// request_contact
// confirm_contact
// process_contact_request
// process_confirm_contact
// parse_contact
// add_contact
// remove_contact
// get_contacts
// get_pending_contacts

// Data
//
// get_data_type
// update_data (locally/remotely)
// set_data
// delete_data (locally/remotely/helper)
// validate
// set_general_validate_callback
// set_validate_callback_for_type
// get_data
// get_all_data_of_type
// get_all_data

// Groups
//
// update_group (locally/remotely) TODO => set_group
// link_group (locally/remotely)
// delete_group (locally/remotely)
// FIXME unlink and delete => just unlink (call delete separately)
// add_child (locally/remotely)
// add_parent (locally/remotely)
// TODO no remove_child (locally/remotely)?
// remove_parent (locally/remotely)

// Permissions
//
// add_admin (locally/remotely)
// remove_admin (locally/remotely)
// add_writer (locally/remotely)
// remove_writer (locally/remotely)
// add_reader
// remove_reader
// TODO => add/remove_privilege_type (using enums for available priv levels)
// add/remove => grant/revoke
// has_privilege

// Sharing
//
// share_data
// unshare_data

pub struct Glue {
  core: Core,
  groups: Groups,
}

impl Glue {
  pub fn new<'a>(
      ip_arg: Option<&'a str>,
      port_arg: Option<&'a str>,
      turn_encryption_off_arg: bool,
  ) -> Glue {
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
    self.core.send_message(dst_idkeys, payload).await;
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
        self.groups.set_group(group_id, group_val);
        Ok(())
      },
      Message::AddParent(group_id, parent_id) => {
        self.groups.add_parent(&group_id, &parent_id).map_err(Error::from)
      },
      Message::AddChild(group_id, child_id) => {
        self.groups.add_child(&group_id, &child_id).map_err(Error::from)
      },
      Message::RemoveParent(group_id, parent_id) => {
        self.groups.remove_parent(&group_id, &parent_id).map_err(Error::from)
      },
      Message::RemoveChild(group_id, child_id) => {
        self.groups.remove_child(&group_id, &child_id).map_err(Error::from)
      },
      Message::LinkGroups(parent_id, child_id) => {
        self.groups.link_groups(&parent_id, &child_id).map_err(Error::from)
      },
      _ => Err(Error::UnknownMessageType),
    }
  }
}

mod tests {
  use crate::glue::{Glue, Message};
  use crate::groups::{Group};

  #[tokio::test]
  async fn test_on_message() {
    let mut glue = Glue::new(None, None, false);

    let group_0 = Group::new(None, None, true, false);
    let group_1 = Group::new(None, None, true, true);

    let dummy_sender = String::from("0");

    let update_group_0_msg = Message::UpdateGroup(
        group_0.group_id().to_string(),
        group_0.clone()
    );

    assert_eq!(Ok(()), glue.on_message(
        &dummy_sender,
        Message::to_string(&update_group_0_msg)
    ).await);
    assert_eq!(
        glue.groups.get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );

    let update_group_1_msg = Message::UpdateGroup(
        group_1.group_id().to_string(),
        group_1.clone()
    );

    assert_eq!(Ok(()), glue.on_message(
        &dummy_sender,
        Message::to_string(&update_group_1_msg)
    ).await);
    assert_eq!(
        glue.groups.get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );

    let add_parent_msg = Message::AddParent(
        group_0.group_id().to_string(),
        group_1.group_id().to_string()
    );

    assert_eq!(Ok(()), glue.on_message(
        &dummy_sender,
        Message::to_string(&add_parent_msg)
    ).await);
    assert_ne!(
        glue.groups.get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );
    assert_eq!(
        glue.groups.get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );

    let remove_parent_msg = Message::RemoveParent(
        group_0.group_id().to_string(),
        group_1.group_id().to_string()
    );

    assert_eq!(Ok(()), glue.on_message(
        &dummy_sender,
        Message::to_string(&remove_parent_msg)
    ).await);
    assert_eq!(
        glue.groups.get_group(group_0.group_id()).unwrap(),
        &group_0.clone()
    );
    assert_eq!(
        glue.groups.get_group(group_1.group_id()).unwrap(),
        &group_1.clone()
    );
  }
}

