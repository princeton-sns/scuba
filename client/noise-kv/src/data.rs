use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

pub trait NoiseData {
    fn data_id(&self) -> &String;
    fn data_type(&self) -> &String;
    fn data_val(&self) -> &String;
    fn group_id(&self) -> &String;
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct BasicData {
    data_id: String,
    data_type: String,
    data_val: String,
    group_id: String,
}

impl BasicData {
    pub fn new(
        data_id: String,
        data_type: String,
        data_val: String,
        group_id: String,
    ) -> BasicData {
        Self {
            data_id,
            data_type,
            data_val,
            group_id,
        }
    }
}

impl NoiseData for BasicData {
    fn data_id(&self) -> &String {
        &self.data_id
    }

    fn data_type(&self) -> &String {
        &self.data_id
    }

    fn data_val(&self) -> &String {
        &self.data_val
    }

    fn group_id(&self) -> &String {
        &self.group_id
    }
}

impl fmt::Display for BasicData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "id: {}, type: {}, group: {}, val: {}",
            self.data_id, self.data_type, self.group_id, self.data_val
        )
    }
}

#[derive(Clone)]
pub struct Validator<T: NoiseData> {
    general_callback: fn(&String, &T) -> bool,
    per_type_callbacks: HashMap<String, fn(&String, &T) -> bool>,
}

fn default_general_callback<T: NoiseData>(
    data_id: &String,
    data_val: &T,
) -> bool {
    if data_id.is_empty() || data_val.data_id().is_empty() {
        return false;
    }
    if data_id != data_val.data_id() {
        return false;
    }
    true
}

impl<T: NoiseData> Validator<T> {
    pub fn new(
        general_callback: Option<fn(&String, &T) -> bool>,
    ) -> Validator<T> {
        Self {
            general_callback: general_callback
                .unwrap_or(default_general_callback),
            per_type_callbacks: HashMap::<String, fn(&String, &T) -> bool>::new(
            ),
        }
    }

    pub fn validate(&self, data_id: &String, data_val: &T) -> bool {
        // call general callback
        if !(self.general_callback)(data_id, data_val) {
            return false;
        }

        // call type-specific callback, if one exists
        match self.per_type_callbacks.get(data_val.data_type()) {
            Some(type_callback) => (type_callback)(data_id, data_val),
            None => true,
        }
    }

    pub fn set_general_validate_callback(
        &mut self,
        callback: fn(&String, &T) -> bool,
    ) {
        self.general_callback = callback;
    }

    pub fn set_validate_callback_for_type(
        &mut self,
        data_type: String,
        callback: fn(&String, &T) -> bool,
    ) -> Option<fn(&String, &T) -> bool> {
        self.per_type_callbacks.insert(data_type, callback)
    }
}

#[derive(Clone)]
pub struct DataStore<T: NoiseData> {
    store: HashMap<String, T>,
    validator: Validator<T>,
}

//fn get_all_data_with_type
impl<T: NoiseData> DataStore<T> {
    pub fn new() -> DataStore<T> {
        Self {
            store: HashMap::<String, T>::new(),
            validator: Validator::<T>::new(None),
        }
    }

    pub fn get_data(&self, data_id: &String) -> Option<&T> {
        self.store.get(data_id)
    }

    pub fn set_data(&mut self, data_id: String, data_val: T) -> Option<T> {
        self.store.insert(data_id, data_val)
    }

    pub fn delete_data(&mut self, data_id: &String) -> Option<T> {
        self.store.remove(data_id)
    }

    pub fn get_all_data(&self) -> &HashMap<String, T> {
        &self.store
    }

    pub fn validate(&self, data_id: &String, data_val: &T) -> bool {
        self.validator.validate(data_id, data_val)
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
        let data = BasicData::new(
            String::from("0"),
            String::from("type"),
            String::from("val"),
            String::from("group"),
        );
        data_store.set_data(data.data_id.to_string(), data.clone());
        assert_eq!(*data_store.get_data(&data.data_id).unwrap(), data);
    }

    #[test]
    fn test_delete_data() {
        let mut data_store = DataStore::new();
        let data = BasicData::new(
            String::from("0"),
            String::from("type"),
            String::from("val"),
            String::from("group"),
        );
        data_store.set_data(data.data_id.to_string(), data.clone());
        data_store.delete_data(&data.data_id);
        assert_eq!(data_store.get_data(&data.data_id), None);
    }

    // TODO test validation
}
