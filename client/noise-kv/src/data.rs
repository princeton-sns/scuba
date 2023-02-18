use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct BasicData {
    data_id: String,
    data_val: String,
}

impl BasicData {
    pub fn new(data_id: String, data_val: String) -> BasicData {
        Self { data_id, data_val }
    }

    fn data_id(&self) -> &String {
        &self.data_id
    }

    fn data_val(&self) -> &String {
        &self.data_val
    }
}

// TODO struct GroupData

//pub trait Data {
//  fn data_id(&self) -> &String;
//  fn get_data_type
//}
//
//impl Data for BasicData {
//  fn data_id(&self) -> &String {
//    &self.data_id
//  }
//}

pub struct Validator {
    general: fn(&String, &BasicData) -> bool,
    //per_type: Option<fn(&BasicData) -> bool>,
}

fn default_general(data_id: &String, data_val: &BasicData) -> bool {
    if data_id.is_empty() || data_val.data_id().is_empty() {
        return false;
    }
    if data_id != data_val.data_id() {
        return false;
    }
    true
}

// validate
// set_general_validate_callback
// set_validate_callback_for_type
impl Validator {
    pub fn new() -> Validator {
        Self {
            general: default_general,
            //per_type: None,
        }
    }

    // TODO make aware of Message types, and let developers make
    // aware of data types?
    // no catch-all general function, but data types whose
    // `per_type` function has not been set there can be a
    // default function that does something similar to
    // `default_general` -> TODO but how to generalize
    // across variable number args? converting to vec would
    // temporarily work, but all `per_type` functions must have
    // the same signature i think.. if we want any
    // enforcement on the types at all, that is (or they can
    // just take in a param that implements some trait, but
    // this is effectively the same as just passing a vec of
    // all args in every time)
    // the goal is to have group validation be like data
    // validation
    pub fn validate(&self, data_id: &String, data_val: &BasicData) -> bool {
        (self.general)(data_id, data_val)
        // TODO also call data-type-specific validation
        // function(s)
    }

    pub fn set_general_validate_callback(
        &mut self,
        callback: fn(&String, &BasicData) -> bool,
    ) {
        self.general = callback;
    }
}

#[derive(Debug, PartialEq)]
pub struct DataStore {
    store: HashMap<String, BasicData>,
    //validator: Validator,
}

//fn get_all_data_of_type
impl DataStore {
    pub fn new() -> DataStore {
        Self {
            store: HashMap::<String, BasicData>::new(),
            //validator: Validator::new(),
        }
    }

    //pub fn validator(&self) -> &Validator {
    //  &self.validator
    //}

    pub fn get_data(&self, data_id: &String) -> Option<&BasicData> {
        self.store.get(data_id)
    }

    pub fn get_data_mut(&mut self, data_id: &String) -> Option<&mut BasicData> {
        self.store.get_mut(data_id)
    }

    pub fn set_data(
        &mut self,
        data_id: String,
        data_val: BasicData,
    ) -> Option<BasicData> {
        self.store.insert(data_id, data_val)
    }

    pub fn delete_data(&mut self, data_id: &String) -> Option<BasicData> {
        self.store.remove(data_id)
    }

    pub fn get_all_data(&self) -> &HashMap<String, BasicData> {
        &self.store
    }
}

mod tests {
    use crate::data::{BasicData, DataStore};
    use std::collections::HashMap;

    #[test]
    fn test_new() {
        assert_eq!(DataStore::new().store, HashMap::<String, BasicData>::new());
    }

    #[test]
    fn test_set_get_data() {
        let mut data_store = DataStore::new();
        let data = BasicData::new(String::from("0"), String::from("val"));
        data_store.set_data(data.data_id().to_string(), data.clone());
        assert_eq!(*data_store.get_data(data.data_id()).unwrap(), data);
    }

    #[test]
    fn test_delete_data() {
        let mut data_store = DataStore::new();
        let data = BasicData::new(String::from("0"), String::from("val"));
        data_store.set_data(data.data_id().to_string(), data.clone());
        data_store.delete_data(data.data_id());
        assert_eq!(data_store.get_data(data.data_id()), None);
    }
}
