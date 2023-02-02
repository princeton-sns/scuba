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
    // set device group
    groups.set_group(idkey.clone(), Group::new(
        Some(idkey.clone()),
        false,
        false
    ));
    groups.link_groups(&linked_name, &idkey);

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

  // TODO user needs to confirm via, e.g. pop-up
  pub fn update_linked_group(
      &mut self,
      sender: String,
      temp_linked_name: String,
      mut members_to_add: HashMap<String, Group>,
  ) -> Result<(), Error> {
    let currently_linked_devices = self.linked_devices();
    let perm_linked_name = self.linked_name().clone();

    let temp_linked_group = members_to_add.get(&temp_linked_name).unwrap().clone();
    members_to_add.remove(&temp_linked_name);

    members_to_add.iter_mut().for_each(|(_, val)| {
      Groups::group_replace(
          val,
          temp_linked_name.clone(),
          perm_linked_name.to_string(),
      );
    });

    // set all groups whose id is not temp_linked_name
    members_to_add.iter_mut().for_each(|(id, val)| {
      self.groups.set_group(id.to_string(), val.clone());
    });

    // merge temp_linked_name group into perm_linked_name group
    for parent in temp_linked_group.parents() {
      self.groups.add_parent(&perm_linked_name, parent);
    }
    for child in temp_linked_group.children().as_ref().unwrap() {
      self.groups.add_child(&perm_linked_name, child);
    }

    Ok(())
  }

  pub fn confirm_update_linked_group(
      &mut self,
      new_linked_name: String,
      new_groups: HashMap<String, Group>,
  ) -> Result<(), Error> {
    self.linked_name = new_linked_name;
    for (group_id, group_val) in new_groups.iter() {
      self.groups.set_group(group_id.to_string(), group_val.clone());
    }

    Ok(())
  }
}

mod tests {
  use crate::devices::Device;
  use crate::groups::{Group, Groups};
  use std::collections::HashSet;

  #[test]
  fn test_new_standalone() {
    let idkey = String::from("0");
    let linked_name = String::from("linked");
    let device = Device::new(idkey.clone(), Some(linked_name.clone()), None);

    let linked_group = device.groups().get_group(&linked_name).unwrap();
    assert_eq!(linked_group.group_id(), &linked_name);
    assert_eq!(linked_group.contact_level(), &false);
    assert_eq!(linked_group.parents(), &HashSet::<String>::new());
    assert_eq!(linked_group.children(), &Some(HashSet::<String>::from([idkey.clone()])));

    let idkey_group = device.groups().get_group(&idkey).unwrap();
    assert_eq!(idkey_group.group_id(), &idkey);
    assert_eq!(idkey_group.contact_level(), &false);
    assert_eq!(idkey_group.parents(), &HashSet::<String>::from([linked_name.clone()]));
    assert_eq!(idkey_group.children(), &None);

    assert_eq!(device.idkey, idkey);
    assert_eq!(device.linked_name, linked_name);
    assert_eq!(device.pending_link_idkey, None);
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

  #[test]
  fn test_update_linked_group() {
    let idkey_0 = String::from("0");
    let mut device_0 = Device::new(idkey_0.clone(), None, None);
    let linked_name_0 = device_0.linked_name().clone();
    let linked_members_0 = device_0.groups().get_all_subgroups(&linked_name_0);

    let idkey_1 = String::from("1");
    let device_1 = Device::new(idkey_1.clone(), None, Some(device_0.linked_name().to_string()));
    let linked_name_1 = device_1.linked_name().clone();
    let linked_members_1 = device_1.groups().get_all_subgroups(&linked_name_1);

    assert_ne!(linked_name_0, linked_name_1);
    assert_ne!(linked_members_0, linked_members_1);
    assert_eq!(linked_members_0.len(), 2);
    assert_eq!(linked_members_1.len(), 2);

    // simulate send and receive of UpdateLinked message
    match device_0.update_linked_group(
        idkey_1.clone(),
        linked_name_1.clone(),
        linked_members_1.clone(),
    ) {
      Ok(_) => println!("Update succeeded"),
      Err(err) => panic!("Error updating linked group: {:?}", err),
    }

    let merged_linked_members = device_0.groups().get_all_subgroups(&linked_name_0);
    assert_eq!(merged_linked_members.len(), 3);

    let merged_linked_group = merged_linked_members.get(&linked_name_0).unwrap();
    assert_eq!(merged_linked_group.group_id(), &linked_name_0);
    assert_eq!(merged_linked_group.parents(), &HashSet::<String>::new());
    assert_eq!(merged_linked_group.children().as_ref(),
        Some(&HashSet::<String>::from([idkey_1.clone(), idkey_0.clone()])));

    let merged_idkey_0_group = merged_linked_members.get(&idkey_0).unwrap();
    assert_eq!(merged_idkey_0_group.group_id(), &idkey_0);
    assert_eq!(merged_idkey_0_group.parents(),
        &HashSet::<String>::from([linked_name_0.clone()]));
    assert_eq!(merged_idkey_0_group.children(), &None);

    let merged_idkey_1_group = merged_linked_members.get(&idkey_1).unwrap();
    assert_eq!(merged_idkey_1_group.group_id(), &idkey_1);
    assert_eq!(merged_idkey_1_group.parents(),
        &HashSet::<String>::from([linked_name_0.clone()]));
    assert_eq!(merged_idkey_1_group.children(), &None);
  }

  #[test]
  fn test_confirm_update_linked() {
    let idkey_0 = String::from("0");
    let mut device_0 = Device::new(idkey_0.clone(), None, None);
    let linked_name_0 = device_0.linked_name().clone();
    let linked_members_0 = device_0.groups().get_all_subgroups(&linked_name_0);

    let idkey_1 = String::from("1");
    let mut device_1 = Device::new(idkey_1.clone(), None, Some(device_0.linked_name().to_string()));
    let linked_name_1 = device_1.linked_name().clone();
    let linked_members_1 = device_1.groups().get_all_subgroups(&linked_name_1);

    // simulate send and receive of UpdateLinked message
    match device_0.update_linked_group(
        idkey_1.clone(),
        linked_name_1.clone(),
        linked_members_1.clone(),
    ) {
      Ok(_) => println!("Update succeeded"),
      Err(err) => panic!("Error updating linked group: {:?}", err),
    }

    // simulate send and receive of ConfirmUpdateLinked message
    match device_1.confirm_update_linked_group(
        linked_name_0.clone(),
        device_0.groups().get_all_groups().clone()
    ) {
      Ok(_) => println!("Update succeeded"),
      Err(err) => panic!("Error confirming update of linked group: {:?}", err),
    }

    let merged_linked_members = device_1.groups().get_all_subgroups(&linked_name_0);
    assert_eq!(merged_linked_members.len(), 3);

    let merged_linked_group = merged_linked_members.get(&linked_name_0).unwrap();
    assert_eq!(merged_linked_group.group_id(), &linked_name_0);
    assert_eq!(merged_linked_group.parents(), &HashSet::<String>::new());
    assert_eq!(merged_linked_group.children().as_ref(),
        Some(&HashSet::<String>::from([idkey_1.clone(), idkey_0.clone()])));

    let merged_idkey_0_group = merged_linked_members.get(&idkey_0).unwrap();
    assert_eq!(merged_idkey_0_group.group_id(), &idkey_0);
    assert_eq!(merged_idkey_0_group.parents(),
        &HashSet::<String>::from([linked_name_0.clone()]));
    assert_eq!(merged_idkey_0_group.children(), &None);

    let merged_idkey_1_group = merged_linked_members.get(&idkey_1).unwrap();
    assert_eq!(merged_idkey_1_group.group_id(), &idkey_1);
    assert_eq!(merged_idkey_1_group.parents(),
        &HashSet::<String>::from([linked_name_0.clone()]));
    assert_eq!(merged_idkey_1_group.children(), &None);
  }
}

