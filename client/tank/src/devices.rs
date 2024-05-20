use parking_lot::RwLock;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use thiserror::Error;

use crate::data::{DataStore, ScubaData};
use crate::metadata::{Group, MetadataStore, PermType, PermissionSet};

#[derive(Debug, PartialEq, Error)]
pub enum Error {
    #[error("Attempted to delete group instead of device.")]
    DeviceHasChildren,
}

#[derive(Clone)]
pub struct Device<T: ScubaData> {
    idkey: Arc<RwLock<String>>,
    pub meta_store: Arc<RwLock<MetadataStore>>,
    pub data_store: Arc<RwLock<DataStore<T>>>,
    pub linked_name: Arc<RwLock<String>>,
    pending_link_idkey: Arc<RwLock<Option<String>>>,
}

// TODO linked_name => root or smthg
impl<T: ScubaData> Device<T> {
    pub fn new(
        idkey: String,
        linked_name_arg: Option<String>,
        pending_link_idkey: Option<String>,
    ) -> Device<T> {
        let linked_name = linked_name_arg.unwrap_or(crate::metadata::generate_uuid());
        let mut meta_store = MetadataStore::new();

        // set linked_group permissions set
        let linked_perm_set = PermissionSet::new(None, None, None, None, None);
        meta_store.set_perm(
            linked_perm_set.perm_id().to_string(),
            linked_perm_set.clone(),
        );

        // set linked group
        meta_store.set_group(
            linked_name.clone(),
            Group::new(
                Some(linked_name.clone()),
                Some(vec![linked_perm_set.perm_id().to_string()]),
                false,
                Some(None),
            ),
        );
        // set device group
        meta_store.set_group(
            idkey.clone(),
            Group::new(
                Some(idkey.clone()),
                Some(vec![linked_perm_set.perm_id().to_string()]),
                false,
                None,
            ),
        );
        // link linked and device groups
        let _ = meta_store.link_groups(&linked_name, &idkey);

        // add linked_group as owner in permissions set
        let _ = meta_store.add_permissions(
            linked_perm_set.perm_id(),
            None,
            PermType::Owners(vec![linked_name.clone()]),
        );

        Self {
            idkey: Arc::new(RwLock::new(idkey)),
            meta_store: Arc::new(RwLock::new(meta_store)),
            data_store: Arc::new(RwLock::new(DataStore::new())),
            linked_name: Arc::new(RwLock::new(linked_name)),
            pending_link_idkey: Arc::new(RwLock::new(pending_link_idkey)),
        }
    }

    pub fn linked_devices_excluding_self(&self) -> Vec<String> {
        self.meta_store
            .read()
            .resolve_group_ids(vec![&self.linked_name.read()])
            .iter()
            .filter(|&x| *x != *self.idkey.read())
            .map(|x| x.clone())
            .collect::<Vec<String>>()
    }

    pub fn linked_devices_excluding_self_and_other(&self, other: &String) -> Vec<String> {
        self.meta_store
            .read()
            .resolve_group_ids(vec![&self.linked_name.read()])
            .iter()
            .filter(|&x| *x != *self.idkey.read() && *x != *other)
            .map(|x| x.clone())
            .collect::<Vec<String>>()
    }

    pub fn linked_devices(&self) -> HashSet<String> {
        self.meta_store
            .read()
            .resolve_group_ids(vec![&self.linked_name.read()])
    }

    // TODO use when linking a pre-existing device to another set of
    // devices
    pub fn get_pending_link_idkey(&self) -> Option<String> {
        self.pending_link_idkey.read().clone()
    }

    //fn set_pending_link_idkey(&self, idkey: String) {
    //    *self.pending_link_idkey.write() = Some(idkey);
    //}

    fn clear_pending_link_idkey(&self) {
        *self.pending_link_idkey.write() = None;
    }

    // TODO user needs to confirm via, e.g. pop-up
    pub fn update_linked_group(
        &self,
        temp_linked_name: String,
        mut members_to_add: HashMap<String, Group>,
    ) -> Result<(), Error> {
        let perm_linked_name = self.linked_name.read().clone();

        let temp_linked_group = members_to_add.get(&temp_linked_name).unwrap().clone();
        members_to_add.remove(&temp_linked_name);

        members_to_add.iter_mut().for_each(|(_, val)| {
            self.meta_store.read().group_replace(
                val,
                temp_linked_name.clone(),
                perm_linked_name.to_string(),
            );
        });

        // set all groups whose id is not temp_linked_name
        members_to_add.iter_mut().for_each(|(id, val)| {
            self.meta_store
                .write()
                .set_group(id.to_string(), val.clone());
        });

        // merge temp_linked_name group into perm_linked_name group
        for parent in temp_linked_group.parents() {
            let _ = self.meta_store
                .write()
                .add_parent(&perm_linked_name, parent);
        }
        for child in temp_linked_group.children().as_ref().unwrap() {
            let _ = self.meta_store.write().add_child(&perm_linked_name, child);
        }

        Ok(())
    }

    pub fn confirm_update_linked_group(
        &self,
        new_linked_name: String,
        new_groups: HashMap<String, Group>,
        new_data: HashMap<String, T>,
    ) -> Result<(), Error> {
        // delete old linked_name
        self.meta_store
            .write()
            .delete_group(&self.linked_name.read().clone());
        *self.linked_name.write() = new_linked_name;

        // add groups
        for (group_id, group_val) in new_groups.iter() {
            self.meta_store
                .write()
                .set_group(group_id.to_string(), group_val.clone());
        }

        // add data
        // into_iter() needed b/c ScubaData can't implement Clone(), so
        // if we want this to stay generic then we need to move data_val
        // from new_data via into_iter() (as opposed to just iter())
        for (data_key, data_val) in new_data.into_iter() {
            self.data_store
                .write()
                .set_data(data_key.to_string(), data_val);
        }

        self.clear_pending_link_idkey();

        Ok(())
    }

    pub fn get_contacts(&self) -> HashSet<String> {
        let mut contacts = HashSet::<String>::new();
        for (id, val) in self.meta_store.read().get_all_groups().iter() {
            if val.is_contact_name {
                contacts.insert(id.to_string());
            }
        }
        contacts
    }

    // TODO get_pending_contacts

    pub fn add_contact(
        &self,
        contact_name: String,
        mut contact_devices: HashMap<String, Group>,
    ) -> Result<(), Error> {
        // TODO is_contact_name will (maybe) be used to add the contact
        // info of clients that we end up communicating with even though
        // they are not directly contacts of ours (necessary for data
        // consistency)

        // for the group whose id == contact_name, set is_contact_name
        // to true
        contact_devices.iter_mut().for_each(|(id, val)| {
            if *id == contact_name {
                val.is_contact_name = true;
            }
            self.meta_store.write().set_group(id.clone(), val.clone());
        });

        Ok(())
    }

    // TODO remove_contact

    // FIXME Currently, this function is unnecessary since none
    // of this data is persistent and will be automatically
    // GC'd when the `device` field of the glue object is
    // set to `None`. But in the future, this function
    // should be used to clean up any related persistent data
    pub fn delete_device(&self, to_delete: String) -> Result<(), Error> {
        let device_group = self
            .meta_store
            .read()
            .get_group(&to_delete)
            .unwrap()
            .clone();
        if device_group.children().as_ref().is_some() {
            return Err(Error::DeviceHasChildren);
        }

        // remove child link to this device from
        // every parent (should have no children)
        for parent in device_group.parents().iter() {
            let _ = self.meta_store.write().remove_child(parent, &to_delete);
        }

        self.meta_store.write().delete_group(&to_delete);

        Ok(())
    }
}

mod tests {
    //use crate::data::BasicData;
    //use crate::devices::Device;
    //use std::collections::HashSet;

    #[test]
    fn test_new_standalone() {
        let idkey = String::from("0");
        let linked_name = String::from("linked");
        let device =
            Device::<BasicData>::new(idkey.clone(), Some(linked_name.clone()), None);

        let meta_store = device.meta_store.read();
        let linked_group = meta_store.get_group(&linked_name).unwrap();
        assert_eq!(linked_group.group_id(), &linked_name);
        assert_eq!(linked_group.is_contact_name, false);
        assert_ne!(linked_group.parents(), &HashSet::<String>::new());
        assert_eq!(
            linked_group.children(),
            &Some(HashSet::<String>::from([idkey.clone()]))
        );

        let idkey_group = meta_store.get_group(&idkey).unwrap();
        assert_eq!(idkey_group.group_id(), &idkey);
        assert_eq!(idkey_group.is_contact_name, false);
        assert_eq!(
            idkey_group.parents(),
            &HashSet::<String>::from([linked_name.clone()])
        );
        assert_eq!(idkey_group.children(), &None);

        assert_eq!(*device.idkey.read(), idkey);
        assert_eq!(*device.linked_name.read(), linked_name);
        assert_eq!(*device.pending_link_idkey.read(), None);
    }

    #[test]
    fn test_get_linked_name() {
        let idkey = String::from("0");
        let linked_name = String::from("linked");
        let device_0 =
            Device::<BasicData>::new(idkey.clone(), Some(linked_name.clone()), None);
        assert_eq!(&*device_0.linked_name.read(), &linked_name);

        let device_1 = Device::<BasicData>::new(idkey, None, None);
        assert_ne!(&*device_1.linked_name.read(), &linked_name);
    }

    #[test]
    fn test_update_linked_group() {
        let idkey_0 = String::from("0");
        let mut device_0 = Device::<BasicData>::new(idkey_0.clone(), None, None);
        let linked_name_0 = device_0.linked_name.read().clone();
        let linked_members_0 =
            device_0.meta_store.read().get_all_subgroups(&linked_name_0);

        let idkey_1 = String::from("1");
        let device_1 = Device::<BasicData>::new(
            idkey_1.clone(),
            None,
            Some(device_0.linked_name.read().clone()),
        );
        let linked_name_1 = device_1.linked_name.read().clone();
        let linked_members_1 =
            device_1.meta_store.read().get_all_subgroups(&linked_name_1);

        assert_ne!(linked_name_0, linked_name_1);
        assert_ne!(linked_members_0, linked_members_1);
        assert_eq!(linked_members_0.len(), 2);
        assert_eq!(linked_members_1.len(), 2);

        // simulate send and receive of UpdateLinked message
        match device_0
            .update_linked_group(linked_name_1.clone(), linked_members_1.clone())
        {
            Ok(_) => println!("Update succeeded"),
            Err(err) => panic!("Error updating linked group: {:?}", err),
        }

        let merged_linked_members =
            device_0.meta_store.read().get_all_subgroups(&linked_name_0);
        assert_eq!(merged_linked_members.len(), 3);

        let merged_linked_group = merged_linked_members.get(&linked_name_0).unwrap();
        assert_eq!(merged_linked_group.group_id(), &linked_name_0);
        assert_ne!(merged_linked_group.parents(), &HashSet::<String>::new());
        assert_eq!(
            merged_linked_group.children().as_ref(),
            Some(&HashSet::<String>::from([idkey_1.clone(), idkey_0.clone()]))
        );

        let merged_idkey_0_group = merged_linked_members.get(&idkey_0).unwrap();
        assert_eq!(merged_idkey_0_group.group_id(), &idkey_0);
        assert_eq!(
            merged_idkey_0_group.parents(),
            &HashSet::<String>::from([linked_name_0.clone()])
        );
        assert_eq!(merged_idkey_0_group.children(), &None);

        let merged_idkey_1_group = merged_linked_members.get(&idkey_1).unwrap();
        assert_eq!(merged_idkey_1_group.group_id(), &idkey_1);
        assert_eq!(
            merged_idkey_1_group.parents(),
            &HashSet::<String>::from([linked_name_0.clone()])
        );
        assert_eq!(merged_idkey_1_group.children(), &None);
    }

    #[test]
    fn test_confirm_update_linked() {
        let idkey_0 = String::from("0");
        let mut device_0 = Device::<BasicData>::new(idkey_0.clone(), None, None);
        let linked_name_0 = device_0.linked_name.read().clone();
        let linked_members_0 =
            device_0.meta_store.read().get_all_subgroups(&linked_name_0);

        let idkey_1 = String::from("1");
        let mut device_1 = Device::<BasicData>::new(
            idkey_1.clone(),
            None,
            Some(device_0.linked_name.read().clone()),
        );
        let linked_name_1 = device_1.linked_name.read().clone();
        let linked_members_1 =
            device_1.meta_store.read().get_all_subgroups(&linked_name_1);

        // simulate send and receive of UpdateLinked message
        match device_0
            .update_linked_group(linked_name_1.clone(), linked_members_1.clone())
        {
            Ok(_) => println!("Update succeeded"),
            Err(err) => panic!("Error updating linked group: {:?}", err),
        }

        // simulate send and receive of ConfirmUpdateLinked message
        match device_1.confirm_update_linked_group(
            linked_name_0.clone(),
            device_0.meta_store.read().get_all_groups().clone(),
            device_0.data_store.read().get_all_data().clone(),
        ) {
            Ok(_) => println!("Update succeeded"),
            Err(err) => {
                panic!("Error confirming update of linked group: {:?}", err)
            }
        }

        assert_eq!(device_1.pending_link_idkey.read().as_ref(), None);

        let merged_linked_members =
            device_1.meta_store.read().get_all_subgroups(&linked_name_0);
        assert_eq!(merged_linked_members.len(), 3);

        let merged_linked_group = merged_linked_members.get(&linked_name_0).unwrap();
        assert_eq!(merged_linked_group.group_id(), &linked_name_0);
        assert_ne!(merged_linked_group.parents(), &HashSet::<String>::new());
        assert_eq!(
            merged_linked_group.children().as_ref(),
            Some(&HashSet::<String>::from([idkey_1.clone(), idkey_0.clone()]))
        );

        let merged_idkey_0_group = merged_linked_members.get(&idkey_0).unwrap();
        assert_eq!(merged_idkey_0_group.group_id(), &idkey_0);
        assert_eq!(
            merged_idkey_0_group.parents(),
            &HashSet::<String>::from([linked_name_0.clone()])
        );
        assert_eq!(merged_idkey_0_group.children(), &None);

        let merged_idkey_1_group = merged_linked_members.get(&idkey_1).unwrap();
        assert_eq!(merged_idkey_1_group.group_id(), &idkey_1);
        assert_eq!(
            merged_idkey_1_group.parents(),
            &HashSet::<String>::from([linked_name_0.clone()])
        );
        assert_eq!(merged_idkey_1_group.children(), &None);
    }

    #[test]
    fn test_delete_self_device() {
        let idkey_0 = String::from("0");
        let mut device_0 = Device::<BasicData>::new(idkey_0.clone(), None, None);
        let linked_name_0 = device_0.linked_name.read().clone();
        let linked_members_0 =
            device_0.meta_store.read().get_all_subgroups(&linked_name_0);

        let idkey_1 = String::from("1");
        let mut device_1 = Device::<BasicData>::new(
            idkey_1.clone(),
            None,
            Some(device_0.linked_name.read().clone()),
        );
        let linked_name_1 = device_1.linked_name.read().clone();
        let linked_members_1 =
            device_1.meta_store.read().get_all_subgroups(&linked_name_1);

        // simulate send and receive of UpdateLinked message
        match device_0
            .update_linked_group(linked_name_1.clone(), linked_members_1.clone())
        {
            Ok(_) => println!("Update succeeded"),
            Err(err) => panic!("Error updating linked group: {:?}", err),
        }

        // simulate send and receive of ConfirmUpdateLinked message
        match device_1.confirm_update_linked_group(
            linked_name_0.clone(),
            device_0.meta_store.read().get_all_groups().clone(),
            device_0.data_store.read().get_all_data().clone(),
        ) {
            Ok(_) => println!("Update succeeded"),
            Err(err) => {
                panic!("Error confirming update of linked group: {:?}", err)
            }
        }

        match device_1.delete_device(idkey_1.clone()) {
            Ok(_) => println!("Delete succeeded"),
            Err(err) => panic!("Error deleting device: {:?}", err),
        }

        let linked_members = device_1.meta_store.read().get_all_subgroups(&linked_name_0);
        assert_eq!(linked_members.len(), 2);

        let linked_group = linked_members.get(&linked_name_0).unwrap();
        assert_eq!(linked_group.group_id(), &linked_name_0);
        assert_ne!(linked_group.parents(), &HashSet::<String>::new());
        assert_eq!(
            linked_group.children().as_ref(),
            Some(&HashSet::<String>::from([idkey_0.clone()]))
        );

        let idkey_0_group = linked_members.get(&idkey_0).unwrap();
        assert_eq!(idkey_0_group.group_id(), &idkey_0);
        assert_eq!(
            idkey_0_group.parents(),
            &HashSet::<String>::from([linked_name_0.clone()])
        );
        assert_eq!(idkey_0_group.children(), &None);

        assert_eq!(None, linked_members.get(&idkey_1));
    }

    #[test]
    fn test_delete_other_device() {
        let idkey_0 = String::from("0");
        let mut device_0 = Device::<BasicData>::new(idkey_0.clone(), None, None);
        let linked_name_0 = device_0.linked_name.read().clone();
        let linked_members_0 =
            device_0.meta_store.read().get_all_subgroups(&linked_name_0);

        let idkey_1 = String::from("1");
        let mut device_1 = Device::<BasicData>::new(
            idkey_1.clone(),
            None,
            Some(device_0.linked_name.read().clone()),
        );
        let linked_name_1 = device_1.linked_name.read().clone();
        let linked_members_1 =
            device_1.meta_store.read().get_all_subgroups(&linked_name_1);

        // simulate send and receive of UpdateLinked message
        match device_0
            .update_linked_group(linked_name_1.clone(), linked_members_1.clone())
        {
            Ok(_) => println!("Update succeeded"),
            Err(err) => panic!("Error updating linked group: {:?}", err),
        }

        // simulate send and receive of ConfirmUpdateLinked message
        match device_1.confirm_update_linked_group(
            linked_name_0.clone(),
            device_0.meta_store.read().get_all_groups().clone(),
            device_0.data_store.read().get_all_data().clone(),
        ) {
            Ok(_) => println!("Update succeeded"),
            Err(err) => {
                panic!("Error confirming update of linked group: {:?}", err)
            }
        }

        match device_0.delete_device(idkey_1.clone()) {
            Ok(_) => println!("Delete succeeded"),
            Err(err) => panic!("Error deleting device: {:?}", err),
        }

        let linked_members = device_0.meta_store.read().get_all_subgroups(&linked_name_0);
        assert_eq!(linked_members.len(), 2);

        let linked_group = linked_members.get(&linked_name_0).unwrap();
        assert_eq!(linked_group.group_id(), &linked_name_0);
        assert_ne!(linked_group.parents(), &HashSet::<String>::new());
        assert_eq!(
            linked_group.children().as_ref(),
            Some(&HashSet::<String>::from([idkey_0.clone()]))
        );

        let idkey_0_group = linked_members.get(&idkey_0).unwrap();
        assert_eq!(idkey_0_group.group_id(), &idkey_0);
        assert_eq!(
            idkey_0_group.parents(),
            &HashSet::<String>::from([linked_name_0.clone()])
        );
        assert_eq!(idkey_0_group.children(), &None);

        assert_eq!(None, linked_members.get(&idkey_1));
    }
}
