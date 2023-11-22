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
    #[error("Permission Set {0} does not exist")]
    PermSetDoesNotExist(String),
}

pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

// TODO make enum (one variant w children, one w/out)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    group_id: String,
    // backpointer to perm for checking if a group has certain permissions
    perm_ids: Vec<String>,
    // FIXME not sure is_contact_name is useful anymore, and kind of defeats
    // the purpose of our "flexible" group structure -> should remove
    pub is_contact_name: bool,
    parents: HashSet<String>,
    children: Option<HashSet<String>>,
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "group_id: {},\n\tperm_ids: {},\n\tis_contact_name: {},\n\tparents: {},\n\tchildren: {}",
            self.group_id,
            itertools::join(self.perm_ids.clone(), ", "),
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
        perm_ids: Option<Vec<String>>,
        is_contact_name: bool,
        // first option specifies whether this is a "node" group (if Some)
        // or a "leaf" group (if None), and the second option specifies
        // if there are any children to add upon creation FIXME this is gross
        children_arg: Option<Option<Vec<String>>>,
    ) -> Group {
        let init_group_id: String;
        if group_id.is_none() {
            init_group_id = generate_uuid();
        } else {
            init_group_id = group_id.unwrap();
        }

        let init_perm_ids: Vec<String>;
        if perm_ids.is_none() {
            init_perm_ids = Vec::<String>::new();
        } else {
            init_perm_ids = perm_ids.unwrap();
        }

        let children = match children_arg {
            Some(option) => match option {
                Some(vec_children) => {
                    Some(HashSet::from_iter(vec_children.into_iter()))
                }
                None => Some(HashSet::<String>::new()),
            },
            None => None,
        };

        Self {
            group_id: init_group_id,
            perm_ids: init_perm_ids,
            is_contact_name,
            parents: HashSet::<String>::new(),
            children,
        }
    }

    pub fn group_id(&self) -> &String {
        &self.group_id
    }

    pub fn perm_ids(&self) -> &Vec<String> {
        &self.perm_ids
    }

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PermType {
    Owners(Vec<String>),
    Writers(Vec<String>),
    Readers(Vec<String>),
    DOReaders(Vec<String>),
}

// TODO make fields pub instead of using getters/setters
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionSet {
    perm_id: String,
    owners: Option<String>,
    writers: Option<String>,
    readers: Option<String>,
    do_readers: Option<String>,
}

impl fmt::Display for PermissionSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "perm_id: {},\n\towner: {},\n\twriters: {},\n\treaders: {},\n\tdo_readers: {}",
            self.perm_id,
            self.owners
                .as_ref()
                .map_or_else(|| "None".to_string(), |s| s.to_string()),
            self.writers
                .as_ref()
                .map_or_else(|| "None".to_string(), |s| s.to_string()),
            self.readers
                .as_ref()
                .map_or_else(|| "None".to_string(), |s| s.to_string()),
            self.do_readers
                .as_ref()
                .map_or_else(|| "None".to_string(), |s| s.to_string()),
        )
    }
}

impl PermissionSet {
    pub fn new(
        perm_id: Option<String>,
        owners: Option<String>,
        writers: Option<String>,
        readers: Option<String>,
        do_readers: Option<String>,
    ) -> PermissionSet {
        let init_perm_id: String;
        if perm_id.is_none() {
            init_perm_id = generate_uuid();
        } else {
            init_perm_id = perm_id.unwrap();
        }

        Self {
            perm_id: init_perm_id,
            owners,
            writers,
            readers,
            do_readers,
        }
    }

    pub fn perm_id(&self) -> &String {
        &self.perm_id
    }

    pub fn owners(&self) -> &Option<String> {
        &self.owners
    }

    pub fn set_owners(&mut self, owner_group_id: &String) -> Option<String> {
        let old_owners = self.owners.clone();
        self.owners = Some(owner_group_id.to_string());
        old_owners
    }

    pub fn writers(&self) -> &Option<String> {
        &self.writers
    }

    pub fn set_writers(&mut self, writer_group_id: &String) -> Option<String> {
        let old_writers = self.writers.clone();
        self.writers = Some(writer_group_id.to_string());
        old_writers
    }

    pub fn readers(&self) -> &Option<String> {
        &self.readers
    }

    pub fn set_readers(&mut self, reader_group_id: &String) -> Option<String> {
        let old_readers = self.readers.clone();
        self.readers = Some(reader_group_id.to_string());
        old_readers
    }

    pub fn do_readers(&self) -> &Option<String> {
        &self.do_readers
    }

    pub fn set_do_readers(
        &mut self,
        do_reader_group_id: &String,
    ) -> Option<String> {
        let old_do_readers = self.do_readers.clone();
        self.do_readers = Some(do_reader_group_id.to_string());
        old_do_readers
    }
}

// TODO maybe, in order to just have a single hashmap, Group vs
// PermissionSet can be in an enum too -> but would this be helpful?
// having two hashmaps is fine

#[derive(Debug, PartialEq, Clone)]
pub struct MetadataStore {
    group_store: HashMap<String, Group>,
    perm_store: HashMap<String, PermissionSet>,
}

impl MetadataStore {
    pub fn new() -> MetadataStore {
        Self {
            group_store: HashMap::<String, Group>::new(),
            perm_store: HashMap::<String, PermissionSet>::new(),
        }
    }

    /*
     * Permission methods
     */

    pub fn get_perm(&self, perm_id: &String) -> Option<&PermissionSet> {
        self.perm_store.get(perm_id)
    }

    pub fn set_perm(
        &mut self,
        perm_id: String,
        perm_val: PermissionSet,
    ) -> Option<PermissionSet> {
        self.perm_store.insert(perm_id, perm_val)
    }

    // TODO
    pub fn add_perm_id_to_subgroups() {}

    pub fn add_permissions(
        &mut self,
        perm_id: &String,
        group_id_opt: Option<String>,
        new_perm_members: PermType,
    ) -> Result<(), Error> {
        if self.get_perm(perm_id).is_none() {
            return Err(Error::PermSetDoesNotExist(perm_id.to_string()));
        }

        let mut perm_set = self.get_perm(perm_id).unwrap().clone();

        let (existing_members, new_members) = match new_perm_members.clone() {
            PermType::Owners(new_owners) => (perm_set.owners(), new_owners),
            PermType::Writers(new_writers) => (perm_set.writers(), new_writers),
            PermType::Readers(new_readers) => (perm_set.readers(), new_readers),
            PermType::DOReaders(new_do_readers) => {
                (perm_set.do_readers(), new_do_readers)
            }
        };

        match existing_members {
            Some(existing_group_id) => {
                // add children to existing group
                match self.get_group(existing_group_id) {
                    Some(existing_group) => {
                        for new_member in new_members.iter() {
                            self.link_groups(existing_group_id, new_member);
                        }
                        // perm_set does not change b/c using existing
                        // group id
                        Ok(())
                    }
                    None => Err(Error::GroupDoesNotExist(
                        existing_group_id.to_string(),
                    )),
                }
            }
            None => {
                // create new group
                let new_group = Group::new(
                    group_id_opt,
                    Some(vec![perm_id.to_string()]),
                    false,
                    Some(Some(new_members.clone())),
                );
                self.set_group(
                    new_group.group_id().to_string(),
                    new_group.clone(),
                );
                for new_member in new_members {
                    self.add_parent(&new_member, new_group.group_id());
                }

                // set perm
                match new_perm_members {
                    PermType::Owners(_) => {
                        perm_set.set_owners(new_group.group_id());
                    }
                    PermType::Writers(_) => {
                        perm_set.set_writers(new_group.group_id());
                    }
                    PermType::Readers(_) => {
                        perm_set.set_readers(new_group.group_id());
                    }
                    PermType::DOReaders(_) => {
                        perm_set.set_do_readers(new_group.group_id());
                    }
                }
                self.set_perm(perm_set.perm_id().to_string(), perm_set);

                Ok(())
            }
        }
    }

    pub fn get_all_perms(&self) -> &HashMap<String, PermissionSet> {
        &self.perm_store
    }

    pub fn has_data_mod_permissions(
        &self,
        group_id: &String,
        perm_id: &String,
    ) -> bool {
        let perm_opt = self.get_perm(perm_id);
        match perm_opt {
            Some(perm_val) => {
                // check if writer or above
                let is_owner = match perm_val.owners() {
                    Some(owner_group) => {
                        self.is_group_member(group_id, owner_group, &None)
                    }
                    None => false,
                };
                // TODO check admins
                let is_writer = match perm_val.writers() {
                    Some(writer_group) => {
                        self.is_group_member(group_id, writer_group, &None)
                    }
                    None => false,
                };
                is_owner || is_writer
            }
            None => false,
        }
    }

    pub fn has_metadata_mod_permissions(
        &self,
        group_id: &String,
        perm_id: &String,
        new_group: Option<Group>,
    ) -> bool {
        let perm_opt = self.get_perm(perm_id);
        match perm_opt {
            // check if admin or above
            Some(perm_val) => {
                match perm_val.owners() {
                    Some(owner_group) => {
                        self.is_group_member(group_id, owner_group, &new_group)
                    }
                    None => false,
                }
                // TODO check admins
            }
            None => false,
        }
    }

    pub fn has_owner_mod_permissions(
        &self,
        group_id: &String,
        perm_id: &String,
    ) -> bool {
        let perm_opt = self.get_perm(perm_id);
        match perm_opt {
            // check if owner
            Some(perm_val) => match perm_val.owners() {
                Some(owner_group) => {
                    self.is_group_member(group_id, owner_group, &None)
                }
                None => false,
            },
            None => false,
        }
    }

    // TODO rename => exists_metadata_mod_permissions
    pub fn find_metadata_mod_permissions(
        &self,
        modifying_group_id: &String,
        to_modify_group_val: Group,
    ) -> bool {
        let perm_ids = to_modify_group_val.perm_ids();
        for perm_id in perm_ids {
            if self.has_metadata_mod_permissions(
                modifying_group_id,
                perm_id,
                Some(to_modify_group_val.clone()),
            ) {
                return true;
            }
        }
        false
    }

    // TODO remove permissions

    /*
     * Group methods
     */

    pub fn get_group(&self, group_id: &String) -> Option<&Group> {
        self.group_store.get(group_id)
    }

    pub fn get_group_mut(&mut self, group_id: &String) -> Option<&mut Group> {
        self.group_store.get_mut(group_id)
    }

    pub fn set_group(
        &mut self,
        group_id: String,
        group_val: Group,
    ) -> Option<Group> {
        self.group_store.insert(group_id, group_val)
    }

    pub fn set_groups(&mut self, groups: HashMap<String, Group>) {
        groups.into_iter().for_each(|(group_id, group_val)| {
            self.set_group(group_id, group_val);
        });
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

        self.group_store.remove(group_id)
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

    pub fn resolve_group_ids<'a>(
        &'a self,
        ids: Vec<&'a String>,
    ) -> HashSet<String> {
        let mut resolved_ids = HashSet::<String>::new();
        let mut visited = HashSet::<&String>::new();

        for id in ids {
            self.resolve_group_ids_helper(&mut resolved_ids, &mut visited, id);
        }

        resolved_ids
    }

    fn resolve_group_ids_helper<'a>(
        &'a self,
        resolved_ids: &mut HashSet<String>,
        visited: &mut HashSet<&'a String>,
        id: &'a String,
    ) {
        let mut to_visit = Vec::<&String>::new();
        to_visit.push(id);

        while !to_visit.is_empty() {
            let cur_id = to_visit.pop().unwrap();

            if visited.get(cur_id).is_some() {
                continue;
            }

            visited.insert(cur_id);
            if let Some(children) = &self.get_group(cur_id).unwrap().children {
                for child in children {
                    to_visit.push(&child);
                }
            } else {
                resolved_ids.insert(cur_id.clone());
            }
        }
    }

    pub fn get_all_groups(&self) -> &HashMap<String, Group> {
        &self.group_store
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
        new_group_opt: &Option<Group>,
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
            match &self.get_group(cur_id) {
                Some(cur_group) => {
                    if let Some(children) = &cur_group.children {
                        for child in children {
                            to_visit.push(&child);
                        }
                    }
                }
                None => match new_group_opt {
                    Some(new_group) => {
                        if let Some(children) = &new_group.children {
                            // children should exist in hashmap since (for now)
                            // we're only adding one group at a time
                            for child in children {
                                to_visit.push(&child);
                            }
                        }
                    }
                    None => panic!("group does not exist"),
                },
            }
        }

        false
    }

    pub fn group_replace(
        &self,
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

/*
mod tests {
    use crate::metadata::{Group, MetadataStore};
    use std::collections::HashMap;
    use std::collections::HashSet;

    #[test]
    fn test_new() {
        assert_eq!(
            MetadataStore::new().group_store,
            HashMap::<String, Group>::new()
        );
    }

    #[test]
    fn test_set_get_group() {
        let group = Group::new(None, true, false);
        let mut meta_store = MetadataStore::new();
        meta_store.set_group(group.group_id.clone(), group.clone());
        assert_eq!(*meta_store.get_group(&group.group_id).unwrap(), group);
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

        let mut meta_store = MetadataStore::new();
        meta_store.set_group(group_0.group_id.clone(), group_0.clone());
        meta_store.set_group(group_1.group_id.clone(), group_1.clone());

        meta_store.link_groups(&group_0.group_id, &group_1.group_id);

        let new_group_0 = meta_store.get_group(&group_0.group_id).unwrap();
        assert_eq!(
            new_group_0.children.as_ref().unwrap(),
            &HashSet::from([group_1.group_id.clone()])
        );

        let new_group_1 = meta_store.get_group(&group_1.group_id).unwrap();
        assert_eq!(
            new_group_1.parents,
            HashSet::from([group_0.group_id.clone()])
        );
    }

    #[test]
    fn test_unlink_groups() {
        let group_0 = Group::new(None, true, true);
        let group_1 = Group::new(None, true, false);

        let mut meta_store = MetadataStore::new();
        meta_store.set_group(group_0.group_id.clone(), group_0.clone());
        meta_store.set_group(group_1.group_id.clone(), group_1.clone());

        meta_store.link_groups(&group_0.group_id, &group_1.group_id);
        meta_store.unlink_groups(&group_0.group_id, &group_1.group_id);

        assert_eq!(&group_0, meta_store.get_group(&group_0.group_id).unwrap());
        assert_eq!(&group_1, meta_store.get_group(&group_1.group_id).unwrap());
    }

    #[test]
    fn test_delete_group() {
        let group = Group::new(None, true, false);
        let mut meta_store = MetadataStore::new();
        meta_store.set_group(group.group_id.clone(), group.clone());
        meta_store.delete_group(&group.group_id);
        assert_eq!(meta_store.get_group(&group.group_id), None);
    }

    #[test]
    fn test_delete_linked_group() {
        let group_0 = Group::new(None, true, true);
        let group_1 = Group::new(None, true, false);

        let mut meta_store = MetadataStore::new();
        meta_store.set_group(group_0.group_id.clone(), group_0.clone());
        meta_store.set_group(group_1.group_id.clone(), group_1.clone());

        meta_store.link_groups(&group_0.group_id, &group_1.group_id);
        meta_store.delete_group(&group_0.group_id);

        assert_eq!(meta_store.get_group(&group_0.group_id), None);
        assert_eq!(meta_store.get_group(&group_1.group_id).unwrap(), &group_1);
    }

    #[test]
    fn test_add_members() {
        let base_group = Group::new(None, true, true);
        let group_0 = Group::new(None, true, false);
        let group_1 = Group::new(None, true, false);
        let group_2 = Group::new(None, true, false);

        let mut meta_store = MetadataStore::new();

        meta_store.set_group(base_group.group_id.clone(), base_group.clone());
        meta_store.set_group(group_0.group_id.clone(), group_0.clone());
        meta_store.set_group(group_1.group_id.clone(), group_1.clone());
        meta_store.set_group(group_2.group_id.clone(), group_2.clone());

        meta_store.add_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_1.group_id(), group_2.group_id()],
        );

        let new_base_group =
            meta_store.get_group(base_group.group_id()).unwrap();
        assert_eq!(
            new_base_group.children.as_ref().unwrap(),
            &HashSet::from([
                group_0.group_id.clone(),
                group_1.group_id.clone(),
                group_2.group_id.clone(),
            ]),
        );

        assert_eq!(
            meta_store.get_group(group_0.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );

        assert_eq!(
            meta_store.get_group(group_1.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );

        assert_eq!(
            meta_store.get_group(group_2.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );
    }

    #[test]
    fn test_remove_members() {
        let base_group = Group::new(None, true, true);
        let group_0 = Group::new(None, true, false);
        let group_1 = Group::new(None, true, false);
        let group_2 = Group::new(None, true, false);

        let mut meta_store = MetadataStore::new();

        meta_store.set_group(base_group.group_id.clone(), base_group.clone());
        meta_store.set_group(group_0.group_id.clone(), group_0.clone());
        meta_store.set_group(group_1.group_id.clone(), group_1.clone());
        meta_store.set_group(group_2.group_id.clone(), group_2.clone());

        meta_store.add_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_1.group_id(), group_2.group_id()],
        );

        meta_store.remove_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_2.group_id()],
        );

        let new_base_group =
            meta_store.get_group(base_group.group_id()).unwrap();
        assert_eq!(
            new_base_group.children.as_ref().unwrap(),
            &HashSet::from([group_1.group_id.clone()]),
        );

        assert_eq!(
            meta_store.get_group(group_0.group_id()).unwrap().parents,
            HashSet::new()
        );

        assert_eq!(
            meta_store.get_group(group_1.group_id()).unwrap().parents,
            HashSet::from([base_group.group_id.clone()])
        );

        assert_eq!(
            meta_store.get_group(group_2.group_id()).unwrap().parents,
            HashSet::new()
        );
    }

    #[test]
    fn test_resolve_group_ids() {
        let base_group = Group::new(None, true, true);
        let group_0 = Group::new(None, true, true);
        let group_0a = Group::new(None, true, false);
        let group_0b = Group::new(None, true, false);
        let group_1 = Group::new(None, true, true);
        let group_1a = Group::new(None, true, false);
        let group_1b = Group::new(None, true, false);

        let mut meta_store = MetadataStore::new();

        meta_store.set_group(base_group.group_id.clone(), base_group.clone());
        meta_store.set_group(group_0.group_id.clone(), group_0.clone());
        meta_store.set_group(group_0a.group_id.clone(), group_0a.clone());
        meta_store.set_group(group_0b.group_id.clone(), group_0b.clone());
        meta_store.set_group(group_1.group_id.clone(), group_1.clone());
        meta_store.set_group(group_1a.group_id.clone(), group_1a.clone());
        meta_store.set_group(group_1b.group_id.clone(), group_1b.clone());

        meta_store.add_members(
            base_group.group_id(),
            vec![group_0.group_id(), group_1.group_id()],
        );

        meta_store.add_members(
            group_0.group_id(),
            vec![group_0a.group_id(), group_0b.group_id()],
        );

        meta_store.add_members(
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
            meta_store.resolve_group_ids(vec![base_group.group_id()]),
            expected_ids
        );

        assert_eq!(
            meta_store.resolve_group_ids(vec![
                group_0.group_id(),
                group_1.group_id()
            ]),
            expected_ids
        );
    }

    #[test]
    fn test_resolve_group_ids_cycles() {}

    #[test]
    fn test_is_member() {}
}
*/
