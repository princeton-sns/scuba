use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, PartialEq, Error)]
pub enum Error {
    #[error("Group {0} has no children")]
    GroupHasNoChildren(String),
    #[error("Group {0} does not exist")]
    GroupDoesNotExist(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    group_id: String,
    pub is_contact_name: bool,
    parents: HashSet<String>,
    children: Option<HashSet<String>>,
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "group_id: {}, is_contact_name: {}, parents: {}, children: {}",
            self.group_id,
            self.is_contact_name,
            itertools::join(self.parents.clone(), ", "),
            self.children.as_ref().map_or_else(
                || "None".to_string(),
                |hs| itertools::join(hs, ", ")
            )
        )
    }
}

impl Group {
    pub fn new(
        group_id: Option<String>,
        is_contact_name: bool,
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
            is_contact_name,
            parents: HashSet::<String>::new(),
            children,
        }
    }

    pub fn new_with_children(
        group_id: Option<String>,
        is_contact_name: bool,
        children: Vec<String>,
    ) -> Group {
        let init_group_id: String;
        if group_id.is_none() {
            init_group_id = Uuid::new_v4().to_string();
        } else {
            init_group_id = group_id.unwrap();
        }

        let children = Some(HashSet::from_iter(children.into_iter()));

        Self {
            group_id: init_group_id,
            is_contact_name,
            parents: HashSet::<String>::new(),
            children,
        }
    }

    pub fn group_id(&self) -> &String {
        &self.group_id
    }

    //pub fn is_contact_name(&self) -> &bool {
    //    &self.is_contact_name
    //}

    //pub fn update_is_contact_name(&mut self, is_contact_name: bool) ->
    // bool {    let old_is_contact_name = self.is_contact_name;
    //    self.is_contact_name = is_contact_name;
    //    old_is_contact_name
    //}

    pub fn parents(&self) -> &HashSet<String> {
        &self.parents
    }

    pub fn add_parent(&mut self, parent_id: String) {
        self.parents.insert(parent_id);
    }

    pub fn remove_parent(&mut self, parent_id: &String) -> bool {
        self.parents.remove(parent_id)
    }

    pub fn children(&self) -> &Option<HashSet<String>> {
        &self.children
    }

    pub fn add_child(&mut self, child_id: String) -> Result<(), Error> {
        match self.children {
            Some(_) => {
                self.children.as_mut().unwrap().insert(child_id);
                Ok(())
            }
            None => Err(Error::GroupHasNoChildren(self.group_id().to_string())),
        }
    }

    pub fn remove_child(&mut self, child_id: &String) -> Result<bool, Error> {
        match self.children {
            Some(_) => Ok(self.children.as_mut().unwrap().remove(child_id)),
            None => Err(Error::GroupHasNoChildren(self.group_id().to_string())),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct GroupStore {
    store: HashMap<String, Group>,
}

impl GroupStore {
    pub fn new() -> GroupStore {
        Self {
            store: HashMap::<String, Group>::new(),
        }
    }

    pub fn get_group(&self, group_id: &String) -> Option<&Group> {
        self.store.get(group_id)
    }

    pub fn get_group_mut(&mut self, group_id: &String) -> Option<&mut Group> {
        self.store.get_mut(group_id)
    }

    pub fn set_group(
        &mut self,
        group_id: String,
        group_val: Group,
    ) -> Option<Group> {
        self.store.insert(group_id, group_val)
    }

    pub fn add_parent(
        &mut self,
        base_group_id: &String,
        to_parent_id: &String,
    ) -> Result<(), Error> {
        if self.get_group(base_group_id).is_none() {
            return Err(Error::GroupDoesNotExist(base_group_id.to_string()));
        }

        if self.get_group(to_parent_id).is_none() {
            return Err(Error::GroupDoesNotExist(to_parent_id.to_string()));
        }

        let mut base_group = self.get_group(base_group_id).unwrap().clone();
        base_group.add_parent(to_parent_id.to_string());
        self.set_group(base_group_id.to_string(), base_group);

        Ok(())
    }

    pub fn remove_parent(
        &mut self,
        base_group_id: &String,
        parent_id: &String,
    ) -> Result<(), Error> {
        if self.get_group(base_group_id).is_none() {
            return Err(Error::GroupDoesNotExist(base_group_id.to_string()));
        }

        if self.get_group(parent_id).is_none() {
            return Err(Error::GroupDoesNotExist(parent_id.to_string()));
        }

        let mut base_group = self.get_group(base_group_id).unwrap().clone();
        base_group.remove_parent(parent_id);
        self.set_group(base_group_id.to_string(), base_group);

        Ok(())
    }

    pub fn add_child(
        &mut self,
        base_group_id: &String,
        to_child_id: &String,
    ) -> Result<(), Error> {
        if self.get_group(base_group_id).is_none() {
            return Err(Error::GroupDoesNotExist(base_group_id.to_string()));
        }

        if self.get_group(to_child_id).is_none() {
            return Err(Error::GroupDoesNotExist(to_child_id.to_string()));
        }

        let mut base_group = self.get_group(base_group_id).unwrap().clone();
        base_group
            .add_child(to_child_id.to_string())
            .map(|_| {
                self.set_group(base_group_id.to_string(), base_group);
                Ok(())
            })
            .unwrap()
    }

    pub fn remove_child(
        &mut self,
        base_group_id: &String,
        child_id: &String,
    ) -> Result<(), Error> {
        if self.get_group(base_group_id).is_none() {
            return Err(Error::GroupDoesNotExist(base_group_id.to_string()));
        }

        if self.get_group(child_id).is_none() {
            return Err(Error::GroupDoesNotExist(child_id.to_string()));
        }

        let mut base_group = self.get_group(base_group_id).unwrap().clone();
        base_group
            .remove_child(child_id)
            .map(|_| {
                self.set_group(base_group_id.to_string(), base_group);
                Ok(())
            })
            .unwrap()
    }

    pub fn link_groups(
        &mut self,
        to_parent_id: &String,
        to_child_id: &String,
    ) -> Result<(), Error> {
        if self.get_group(to_parent_id).is_none() {
            return Err(Error::GroupDoesNotExist(to_parent_id.to_string()));
        }

        if self.get_group(to_child_id).is_none() {
            return Err(Error::GroupDoesNotExist(to_child_id.to_string()));
        }

        // set child of to_parent group
        let mut to_parent_group = self.get_group(to_parent_id).unwrap().clone();
        if to_parent_group.children.is_none() {
            return Err(Error::GroupHasNoChildren(to_parent_id.to_string()));
        }
        to_parent_group.add_child(to_child_id.to_string());
        self.set_group(to_parent_id.to_string(), to_parent_group);

        // set parent of to_child group
        let mut to_child_group = self.get_group(to_child_id).unwrap().clone();
        to_child_group.add_parent(to_parent_id.to_string());
        self.set_group(to_child_id.to_string(), to_child_group);

        Ok(())
    }

    pub fn unlink_groups(
        &mut self,
        parent_id: &String,
        child_id: &String,
    ) -> Result<(), Error> {
        if self.get_group(parent_id).is_none() {
            return Err(Error::GroupDoesNotExist(parent_id.to_string()));
        }

        if self.get_group(child_id).is_none() {
            return Err(Error::GroupDoesNotExist(child_id.to_string()));
        }

        // unset child of parent group
        let mut parent_group = self.get_group(parent_id).unwrap().clone();
        if parent_group.children.is_none() {
            return Err(Error::GroupHasNoChildren(parent_id.to_string()));
        }
        parent_group.remove_child(child_id);
        self.set_group(parent_id.to_string(), parent_group);

        // unset parent of child group
        let mut child_group = self.get_group(child_id).unwrap().clone();
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
            let mut parent_group = self.get_group(&parent_id).unwrap().clone();
            parent_group.remove_child(group_id);
            self.set_group(parent_id.to_string(), parent_group);
        }

        // delete from any childrens' parents lists
        if let Some(children) = group_val.children {
            for child_id in children {
                let mut child_group =
                    self.get_group(&child_id).unwrap().clone();
                child_group.remove_parent(group_id);
                self.set_group(child_id.to_string(), child_group);
            }
        }

        self.store.remove(group_id)
    }

    pub fn is_device_group(&self, group_val: &Group) -> bool {
        if group_val.children.is_none() {
            return true;
        }
        false
    }

    pub fn add_members(
        &mut self,
        base_group_id: &String,
        ids_to_add: Vec<&String>,
    ) {
        for id_to_add in ids_to_add {
            self.link_groups(base_group_id, id_to_add);
        }
    }

    pub fn remove_members(
        &mut self,
        base_group_id: &String,
        ids_to_remove: Vec<&String>,
    ) {
        for id_to_remove in ids_to_remove {
            self.unlink_groups(base_group_id, id_to_remove);
        }
    }

    pub fn resolve_ids(&self, ids: Vec<String>) -> HashSet<String> {
        let mut resolved_ids = HashSet::<String>::new();
        let mut visited = HashSet::<String>::new();

        for id in ids {
            self.resolve_ids_helper(&mut resolved_ids, &mut visited, id);
        }

        resolved_ids
    }

    fn resolve_ids_helper(
        &self,
        resolved_ids: &mut HashSet<String>,
        visited: &mut HashSet<String>,
        id: String,
    ) {
        let mut to_visit = Vec::<String>::new();
        to_visit.push(id);

        while !to_visit.is_empty() {
            let cur_id = to_visit.pop().unwrap();

            if visited.get(&cur_id).is_some() {
                continue;
            }

            visited.insert(cur_id.clone());
            if let Some(children) = &self.get_group(&cur_id).unwrap().children {
                for child in children {
                    to_visit.push(child.clone());
                }
            } else {
                resolved_ids.insert(cur_id.clone());
            }
        }
    }

    pub fn get_all_groups(&self) -> &HashMap<String, Group> {
        &self.store
    }

    pub fn get_all_subgroups<'a>(
        &'a self,
        group_id: &'a String,
    ) -> HashMap<String, Group> {
        let mut subgroups = HashMap::<String, Group>::new();
        let mut visited = HashSet::<&String>::new();
        let mut to_visit = Vec::<&String>::new();
        to_visit.push(group_id);

        while !to_visit.is_empty() {
            let cur_id = to_visit.pop().unwrap();

            if visited.get(cur_id).is_some() {
                continue;
            }
            visited.insert(cur_id);

            let cur_val = self.get_group(cur_id).unwrap();
            subgroups.insert(cur_id.to_string(), cur_val.clone());

            if let Some(children) = &cur_val.children {
                for child in children {
                    to_visit.push(&child);
                }
            }
        }

        subgroups
    }

    pub fn is_group_member<'a>(
        &'a self,
        is_member_id: &'a String,
        group_id: &'a String,
    ) -> bool {
        let mut visited = HashSet::<&String>::new();
        let mut to_visit = Vec::<&String>::new();
        to_visit.push(group_id);

        while !to_visit.is_empty() {
            let cur_id = to_visit.pop().unwrap();

            if visited.get(cur_id).is_some() {
                continue;
            }

            if cur_id == is_member_id {
                return true;
            }

            visited.insert(cur_id);
            if let Some(children) = &self.get_group(cur_id).unwrap().children {
                for child in children {
                    to_visit.push(&child);
                }
            }
        }

        false
    }

    pub fn group_replace(
        &self, // FIXME use self?
        group: &mut Group,
        id_to_replace: String,
        replacement_id: String,
    ) {
        if group.group_id() == &id_to_replace {
            group.group_id = replacement_id.clone();
        }
        if group.remove_parent(&id_to_replace) {
            group.add_parent(replacement_id.clone());
        }
        group.remove_child(&id_to_replace).map(|result| {
            if result {
                group.add_child(replacement_id);
            }
        });
    }

    // FIXME use self?
    pub fn group_contains(&self, group: &Group, id_to_check: String) -> bool {
        if group.group_id() == &id_to_check {
            return true;
        }
        if group.parents().contains(&id_to_check) {
            return true;
        }
        match group.children() {
            Some(children) => {
                if children.contains(&id_to_check) {
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

mod tests {
    use crate::groups::{Group, GroupStore};
    use std::collections::HashMap;
    use std::collections::HashSet;

    #[test]
    fn test_new() {
        assert_eq!(GroupStore::new().store, HashMap::<String, Group>::new());
    }

    #[test]
    fn test_set_get_group() {
        let group = Group::new(None, true, false);
        let mut group_store = GroupStore::new();
        group_store.set_group(group.group_id.clone(), group.clone());
        assert_eq!(*group_store.get_group(&group.group_id).unwrap(), group);
    }

    #[test]
    fn test_modify_group_parents() {
        let mut group_0 = Group::new(None, true, false);
        let group_1 = Group::new(None, true, true);

        group_0.add_parent(group_1.group_id.clone());
        assert_eq!(group_0.parents, HashSet::from([group_1.group_id.clone()]));

        group_0.remove_parent(&group_1.group_id.clone());
        assert_eq!(group_0.parents, HashSet::new());
    }

    #[test]
    fn test_modify_group_children() {
        let mut group_0 = Group::new(None, true, true);
        let group_1 = Group::new(None, true, false);

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
        let group_0 = Group::new(None, true, true);
        let group_1 = Group::new(None, true, false);

        let mut group_store = GroupStore::new();
        group_store.set_group(group_0.group_id.clone(), group_0.clone());
        group_store.set_group(group_1.group_id.clone(), group_1.clone());

        group_store.link_groups(&group_0.group_id, &group_1.group_id);

        let new_group_0 = group_store.get_group(&group_0.group_id).unwrap();
        assert_eq!(
            new_group_0.children.as_ref().unwrap(),
            &HashSet::from([group_1.group_id.clone()])
        );

        let new_group_1 = group_store.get_group(&group_1.group_id).unwrap();
        assert_eq!(
            new_group_1.parents,
            HashSet::from([group_0.group_id.clone()])
        );
    }

    #[test]
    fn test_unlink_groups() {
        let group_0 = Group::new(None, true, true);
        let group_1 = Group::new(None, true, false);

        let mut group_store = GroupStore::new();
        group_store.set_group(group_0.group_id.clone(), group_0.clone());
        group_store.set_group(group_1.group_id.clone(), group_1.clone());

        group_store.link_groups(&group_0.group_id, &group_1.group_id);
        group_store.unlink_groups(&group_0.group_id, &group_1.group_id);

        assert_eq!(&group_0, group_store.get_group(&group_0.group_id).unwrap());
        assert_eq!(&group_1, group_store.get_group(&group_1.group_id).unwrap());
    }

    #[test]
    fn test_delete_group() {
        let group = Group::new(None, true, false);
        let mut group_store = GroupStore::new();
        group_store.set_group(group.group_id.clone(), group.clone());
        group_store.delete_group(&group.group_id);
        assert_eq!(group_store.get_group(&group.group_id), None);
    }

    #[test]
    fn test_delete_linked_group() {
        let group_0 = Group::new(None, true, true);
        let group_1 = Group::new(None, true, false);

        let mut group_store = GroupStore::new();
        group_store.set_group(group_0.group_id.clone(), group_0.clone());
        group_store.set_group(group_1.group_id.clone(), group_1.clone());

        group_store.link_groups(&group_0.group_id, &group_1.group_id);
        group_store.delete_group(&group_0.group_id);

        assert_eq!(group_store.get_group(&group_0.group_id), None);
        assert_eq!(group_store.get_group(&group_1.group_id).unwrap(), &group_1);
    }

    #[test]
    fn test_add_members() {
        let base_group = Group::new(None, true, true);
        let group_0 = Group::new(None, true, false);
        let group_1 = Group::new(None, true, false);
        let group_2 = Group::new(None, true, false);

        let mut group_store = GroupStore::new();

        group_store.set_group(base_group.group_id.clone(), base_group.clone());
        group_store.set_group(group_0.group_id.clone(), group_0.clone());
        group_store.set_group(group_1.group_id.clone(), group_1.clone());
        group_store.set_group(group_2.group_id.clone(), group_2.clone());

        group_store.add_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_1.group_id(), group_2.group_id()],
        );

        let new_base_group =
            group_store.get_group(base_group.group_id()).unwrap();
        assert_eq!(
            new_base_group.children.as_ref().unwrap(),
            &HashSet::from([
                group_0.group_id.clone(),
                group_1.group_id.clone(),
                group_2.group_id.clone(),
            ]),
        );

        assert_eq!(
            group_store.get_group(group_0.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );

        assert_eq!(
            group_store.get_group(group_1.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );

        assert_eq!(
            group_store.get_group(group_2.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );
    }

    #[test]
    fn test_remove_members() {
        let base_group = Group::new(None, true, true);
        let group_0 = Group::new(None, true, false);
        let group_1 = Group::new(None, true, false);
        let group_2 = Group::new(None, true, false);

        let mut group_store = GroupStore::new();

        group_store.set_group(base_group.group_id.clone(), base_group.clone());
        group_store.set_group(group_0.group_id.clone(), group_0.clone());
        group_store.set_group(group_1.group_id.clone(), group_1.clone());
        group_store.set_group(group_2.group_id.clone(), group_2.clone());

        group_store.add_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_1.group_id(), group_2.group_id()],
        );

        group_store.remove_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_2.group_id()],
        );

        let new_base_group =
            group_store.get_group(base_group.group_id()).unwrap();
        assert_eq!(
            new_base_group.children.as_ref().unwrap(),
            &HashSet::from([group_1.group_id.clone()]),
        );

        assert_eq!(
            group_store.get_group(group_0.group_id()).unwrap().parents,
            HashSet::new()
        );

        assert_eq!(
            group_store.get_group(group_1.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );

        assert_eq!(
            group_store.get_group(group_2.group_id()).unwrap().parents,
            HashSet::new()
        );
    }

    #[test]
    fn test_resolve_ids() {
        let base_group = Group::new(None, true, true);
        let group_0 = Group::new(None, true, true);
        let group_0a = Group::new(None, true, false);
        let group_0b = Group::new(None, true, false);
        let group_1 = Group::new(None, true, true);
        let group_1a = Group::new(None, true, false);
        let group_1b = Group::new(None, true, false);

        let mut group_store = GroupStore::new();

        group_store.set_group(base_group.group_id.clone(), base_group.clone());
        group_store.set_group(group_0.group_id.clone(), group_0.clone());
        group_store.set_group(group_0a.group_id.clone(), group_0a.clone());
        group_store.set_group(group_0b.group_id.clone(), group_0b.clone());
        group_store.set_group(group_1.group_id.clone(), group_1.clone());
        group_store.set_group(group_1a.group_id.clone(), group_1a.clone());
        group_store.set_group(group_1b.group_id.clone(), group_1b.clone());

        group_store.add_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_1.group_id()],
        );

        group_store.add_members(
            group_0.group_id(),
            vec![group_0a.group_id(), group_0b.group_id()],
        );

        group_store.add_members(
            group_1.group_id(),
            vec![group_1a.group_id(), group_1b.group_id()],
        );

        let expected_ids = HashSet::from([
            group_0a.group_id().clone(),
            group_0b.group_id().clone(),
            group_1a.group_id().clone(),
            group_1b.group_id().clone(),
        ]);

        assert_eq!(
            group_store.resolve_ids(vec![base_group.group_id()]),
            expected_ids
        );

        assert_eq!(
            group_store
                .resolve_ids(vec![group_0.group_id(), group_1.group_id()]),
            expected_ids
        );
    }

    #[test]
    fn test_resolve_ids_cycles() {}

    #[test]
    fn test_is_member() {}
}
