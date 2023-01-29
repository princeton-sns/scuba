use std::collections::HashMap;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug)]
pub enum Error {
  GroupHasNoChildren,
  GroupDoesNotExist,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
  group_id: String,
  group_name: Option<String>,
  contact_level: bool,
  parents: HashSet<String>,
  children: Option<HashSet<String>>,
}

impl Group {
  pub fn new(
      group_id: Option<String>,
      group_name: Option<String>,
      contact_level: bool,
      init_children: bool,
  ) -> Group {
    let init_group_id: String;
    if group_id.is_none() {
      init_group_id = Uuid::new_v4().to_string();
    } else {
      init_group_id = group_id.unwrap();
    }

    let mut children = None;
    if init_children {
      children = Some(HashSet::<String>::new());
    }

    Self {
      group_id: init_group_id,
      group_name,
      contact_level,
      parents: HashSet::<String>::new(),
      children,
    }
  }

  pub fn group_id(&self) -> &String {
    &self.group_id
  }

  pub fn group_name(&self) -> &Option<String> {
    &self.group_name
  }

  pub fn update_group_name(
      &mut self,
      name: Option<String>
  ) -> Option<String> {
    let old_name = self.group_name.clone();
    self.group_name = name;
    old_name
  }

  pub fn contact_level(&self) -> &bool {
    &self.contact_level
  }

  pub fn update_contact_level(
      &mut self,
      contact_level: bool,
  ) -> bool {
    let old_contact_level = self.contact_level;
    self.contact_level = contact_level;
    old_contact_level
  }

  pub fn parents(&self) -> &HashSet<String> {
    &self.parents
  }

  pub fn add_parent(&mut self, parent_id: String) {
    self.parents.insert(parent_id);
  }

  pub fn remove_parent(&mut self, parent_id: &String) {
    self.parents.remove(parent_id);
  }

  pub fn children(&self) -> &Option<HashSet<String>> {
    &self.children
  }

  pub fn add_child(&mut self, child_id: String) -> Result<(), Error> {
    match self.children {
      Some(_) => {
        self.children.as_mut().unwrap().insert(child_id);
        Ok(())
      },
      None => Err(Error::GroupHasNoChildren),
    }
  }

  pub fn remove_child(&mut self, child_id: &String) -> Result<(), Error> {
    match self.children {
      Some(_) => {
        self.children.as_mut().unwrap().remove(child_id);
        Ok(())
      },
      None => Err(Error::GroupHasNoChildren),
    }
  }
}

#[derive(Debug, PartialEq)]
pub struct Groups {
  stored: HashMap<String, Group>,
}

impl Groups {
  pub fn new() -> Groups {
    Self {
      stored: HashMap::<String, Group>::new(),
    }
  }

  pub fn get_group(&self, group_id: &String) -> Option<&Group> {
    self.stored.get(group_id)
  }

  pub fn get_group_mut(
      &mut self,
      group_id: &String
  ) -> Option<&mut Group> {
    self.stored.get_mut(group_id)
  }

  pub fn set_group(
      &mut self,
      group_id: String,
      group_val: Group
  ) -> Option<Group> {
    self.stored.insert(group_id, group_val)
  }

  pub fn link_groups(
      &mut self,
      to_parent_id: &String,
      to_child_id: &String,
  ) -> Result<(), Error> {
    if self.get_group(to_parent_id).is_none()
        && self.get_group(to_child_id).is_none() {
      return Err(Error::GroupDoesNotExist);
    }

    // set child of to_parent group
    let mut to_parent_group = self.get_group_mut(to_parent_id).unwrap().clone();
    if to_parent_group.children.is_none() {
      return Err(Error::GroupHasNoChildren);
    }
    to_parent_group.add_child(to_child_id.to_string());
    self.set_group(to_parent_id.to_string(), to_parent_group);

    // set parent of to_child group
    let mut to_child_group = self.get_group_mut(to_child_id).unwrap().clone();
    to_child_group.add_parent(to_parent_id.to_string());
    self.set_group(to_child_id.to_string(), to_child_group);

    Ok(())
  }

  pub fn unlink_groups(
      &mut self,
      parent_id: &String,
      child_id: &String,
  ) -> Result<(), Error> {
    if self.get_group(parent_id).is_none()
        && self.get_group(child_id).is_none() {
      return Err(Error::GroupDoesNotExist);
    }

    // unset child of parent group
    let mut parent_group = self.get_group_mut(parent_id).unwrap().clone();
    if parent_group.children.is_none() {
      return Err(Error::GroupHasNoChildren);
    }
    parent_group.remove_child(child_id);
    self.set_group(parent_id.to_string(), parent_group);

    // unset parent of child group
    let mut child_group = self.get_group_mut(child_id).unwrap().clone();
    child_group.remove_parent(parent_id);
    self.set_group(child_id.to_string(), child_group);

    Ok(())
  }

  pub fn delete_group(&mut self, group_id: &String) -> Option<Group> {
    if self.get_group(group_id).is_none() {
      return None;
    }

    let group_val = self.get_group(group_id).unwrap().clone();

    // delete from all parents' children lists
    for parent_id in &group_val.parents {
      let mut parent_group = self.get_group_mut(&parent_id).unwrap().clone();
      parent_group.remove_child(group_id);
      self.set_group(parent_id.to_string(), parent_group);
    }

    // delete from any childrens' parents lists
    if let Some(children) = group_val.children {
      for child_id in children {
        let mut child_group = self.get_group_mut(&child_id).unwrap().clone();
        child_group.remove_parent(group_id);
        self.set_group(child_id.to_string(), child_group);
      }
    }

    self.stored.remove(group_id)
  }

  pub fn is_device_group(&self, group_val: &Group) -> bool {
    if group_val.children.is_none() {
      return true;
    }
    false
  }
}

mod tests {
  use std::collections::HashMap;
  use std::collections::HashSet;
  use crate::groups::{Group, Groups};

  #[test]
  fn test_new() {
    assert_eq!(Groups::new().stored, HashMap::<String, Group>::new());
  }

  #[test]
  fn test_set_get_group() {
    let group = Group::new(None, None, true, false);
    let mut groups = Groups::new();
    groups.set_group(group.group_id.clone(), group.clone());
    assert_eq!(*groups.get_group(&group.group_id).unwrap(), group);
  }

  #[test]
  fn test_modify_group_parents() {
    let mut group_0 = Group::new(None, None, true, false);
    let group_1 = Group::new(None, None, true, true);

    group_0.add_parent(group_1.group_id.clone());
    assert_eq!(
        group_0.parents,
        HashSet::from([group_1.group_id.clone()])
    );

    group_0.remove_parent(&group_1.group_id.clone());
    assert_eq!(group_0.parents, HashSet::new());
  }

  #[test]
  fn test_modify_group_children() {
    let mut group_0 = Group::new(None, None, true, true);
    let group_1 = Group::new(None, None, true, false);

    group_0.add_child(group_1.group_id.clone());
    assert_eq!(
        group_0.children.as_ref().unwrap(),
        &HashSet::from([group_1.group_id.clone()])
    );

    group_0.remove_child(&group_1.group_id.clone());
    assert_eq!(group_0.children.unwrap(), HashSet::new());
  }

  #[test]
  fn test_link_groups() {
    let group_0 = Group::new(None, None, true, true);
    let group_1 = Group::new(None, None, true, false);

    let mut groups = Groups::new();
    groups.set_group(group_0.group_id.clone(), group_0.clone());
    groups.set_group(group_1.group_id.clone(), group_1.clone());

    groups.link_groups(&group_0.group_id, &group_1.group_id);

    let new_group_0 = groups.get_group(&group_0.group_id).unwrap();
    assert_eq!(
        new_group_0.children.as_ref().unwrap(),
        &HashSet::from([group_1.group_id.clone()])
    );

    let new_group_1 = groups.get_group(&group_1.group_id).unwrap();
    assert_eq!(
        new_group_1.parents,
        HashSet::from([group_0.group_id.clone()])
    );
  }

  #[test]
  fn test_unlink_groups() {
    let group_0 = Group::new(None, None, true, true);
    let group_1 = Group::new(None, None, true, false);

    let mut groups = Groups::new();
    groups.set_group(group_0.group_id.clone(), group_0.clone());
    groups.set_group(group_1.group_id.clone(), group_1.clone());

    groups.link_groups(&group_0.group_id, &group_1.group_id);
    groups.unlink_groups(&group_0.group_id, &group_1.group_id);

    assert_eq!(&group_0, groups.get_group(&group_0.group_id).unwrap());
    assert_eq!(&group_1, groups.get_group(&group_1.group_id).unwrap());
  }

  #[test]
  fn test_delete_group() {
    let group = Group::new(None, None, true, false);
    let mut groups = Groups::new();
    groups.set_group(group.group_id.clone(), group.clone());
    groups.delete_group(&group.group_id);
    assert_eq!(groups.get_group(&group.group_id), None);
  }

  #[test]
  fn test_delete_linked_group() {
    let group_0 = Group::new(None, None, true, true);
    let group_1 = Group::new(None, None, true, false);

    let mut groups = Groups::new();
    groups.set_group(group_0.group_id.clone(), group_0.clone());
    groups.set_group(group_1.group_id.clone(), group_1.clone());

    groups.link_groups(&group_0.group_id, &group_1.group_id);
    groups.delete_group(&group_0.group_id);

    assert_eq!(groups.get_group(&group_0.group_id), None);
    assert_eq!(groups.get_group(&group_1.group_id).unwrap(), &group_1);
  }
}

