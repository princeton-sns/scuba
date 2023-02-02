use std::collections::HashSet;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::groups::{Group, Groups};

// Devices
//
// delete_current_device
// delete_linked_device
// delete_all_devices
// delete_device (locally/remotely)
//
// confirm_update_linked -> send_confirm_updated_linked
// process_confirm_update_linked -> confirm_updated_linked

#[derive(Debug, PartialEq, Error)]
pub enum Error {
  #[error("temp")]
  Temp,
}

#[derive(Debug, PartialEq)]
pub struct Device {
  idkey: String, // FIXME need here?
  groups: Groups,
  linked_name: String,
  pending_link_idkey: Option<String>,
}

impl Device {
  pub fn new(
      idkey: String,
      linked_name_arg: Option<String>,
      pending_link_idkey: Option<String>
  ) -> Device {
    let linked_name = linked_name_arg.unwrap_or(Uuid::new_v4().to_string());
    let mut groups = Groups::new();

    // set linked group
    groups.set_group(linked_name.clone(), Group::new(
        Some(linked_name.clone()),
        false,
        true
    ));
    groups.add_child(&linked_name, &idkey);

    // set device group
    groups.set_group(idkey.clone(), Group::new(
        Some(idkey.clone()),
        false,
        false
    ));
    groups.add_parent(&idkey, &linked_name);

    Self {
      idkey,
      groups,
      linked_name,
      pending_link_idkey,
    }
  }

  pub fn linked_name(&self) -> &String {
    &self.linked_name
  }

  pub fn linked_devices(&self) -> HashSet<&String> {
    self.groups().resolve_ids(vec![self.linked_name()])
  }

  pub fn groups(&self) -> &Groups {
    &self.groups
  }

  pub fn groups_mut(&mut self) -> &mut Groups {
    &mut self.groups
  }

  fn set_pending_link_idkey(&mut self, idkey: String) {
    self.pending_link_idkey = Some(idkey);
  }

  fn get_pending_link_idkey(&self) -> &Option<String> {
    &self.pending_link_idkey
  }

  fn clear_pending_link_idkey(&mut self) {
    self.pending_link_idkey = None;
  }

  pub async fn update_linked_group(
      &mut self,
      sender: String,
      temp_linked_name: String,
      mut members_to_add: HashMap<String, Group>,
  ) -> Result<(), Error> {
    // TODO user needs to confirm via, e.g. pop-up
    let currently_linked_devices = self.linked_devices();
    let perm_linked_name = self.linked_name();

    println!("IN UPDATE_LINKED_GROUP");
    println!("members_to_add: {:?}", members_to_add.clone());

    members_to_add.iter_mut().map(|(_, val)| {
      Groups::group_replace(
          val,
          temp_linked_name.clone(),
          perm_linked_name.to_string(),
      );
    });

    // merge new members into groups
    members_to_add.iter_mut().map(|(id, val)| {
      println!("s");
    });

    Ok(())
  }
}

mod tests {
  use crate::devices::Device;
  use crate::groups::{Group, Groups};

  #[test]
  fn test_new_standalone() {
    let idkey = String::from("0");
    let linked_name = String::from("linked");
    let device = Device::new(idkey.clone(), Some(linked_name.clone()), None);

    let mut groups = Groups::new();
    groups.set_group(linked_name.clone(), Group::new(
        Some(linked_name.clone()),
        false,
        true
    ));
    groups.add_child(&linked_name, &idkey);
    groups.set_group(idkey.clone(), Group::new(
        Some(idkey.clone()),
        false,
        false
    ));
    groups.add_parent(&idkey, &linked_name);

    assert_eq!(device, Device {
      idkey,
      linked_name,
      groups,
      pending_link_idkey: None,
    });
  }

  #[test]
  fn test_get_linked_name() {
    let idkey = String::from("0");
    let linked_name = String::from("linked");
    let device_0 = Device::new(idkey.clone(), Some(linked_name.clone()), None);
    assert_eq!(device_0.linked_name(), &linked_name);

    let device_1 = Device::new(idkey, None, None);
    assert_ne!(device_1.linked_name(), &linked_name);
  }

  #[tokio::test]
  async fn test_update_linked_group() {
    let idkey_0 = String::from("0");
    let device_0 = Device::new(idkey_0, None, None);
    let linked_name_0 = device_0.linked_name();
    let linked_members_0 = device_0.groups().get_all_subgroups(linked_name_0);
    println!("groups_0: {:#?}", device_0.groups());
    println!("linked_members_0: {:#?}", linked_members_0);

    //let idkey_1 = String::from("1");
    //let device_1 = Device::new(idkey_1, Some(device_0.linked_name().to_string()), None);
    //let linked_name_1 = device_1.linked_name();
    //let linked_members_1 = device_1.groups().get_all_subgroups(linked_name_1);
    //println!("linked_members_1: {:?}", linked_members_1);
  }
}

