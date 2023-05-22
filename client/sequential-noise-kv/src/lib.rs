#![feature(async_closure)]

// TODO client -> driver, devices -> client
pub mod client;
pub mod data;
pub mod devices;
pub mod metadata;

use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;

#[no_mangle]
pub extern "system" fn Java_com_example_noiseperiodtracker_MainActivity_createStandaloneDevice<
    'local,
>(
    mut env: JNIEnv<'local>,
    class: JClass<'local>,
) { //-> jni::objects::JValueGen {
     //let client_obj = client::NoiseKVClient::new(None, None, false,
     // None, None).await; client_obj.create_standalone_device();
}
