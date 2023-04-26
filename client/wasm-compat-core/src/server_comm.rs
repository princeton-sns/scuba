use crate::core::CoreClient;
use std::marker::PhantomData;
// Using http for sending requests
//use http::Request;
// TODO Use something else for receiving requests
//use eventsource_client::{Client, ClientBuilder, SSE};

//use std::sync::Arc;
use url::Url;

const BOOTSTRAP_SERVER_URL: &'static str = "http://localhost:8081";

//pub struct ServerComm<'a, C: CoreClient> {
pub struct ServerComm<'a> {
    idkey: &'a str,
    //_pd: PhantomData<C>,
}

//impl<'a, C: CoreClient> ServerComm<'a, C> {
impl<'a> ServerComm<'a> {
    pub fn new(
        //ip_arg: Option<&'a str>,
        //port_arg: Option<&'a str>,
        idkey: &'a str,
        //core_option: Option<Arc<Core<C>>>,
    ) -> Self {
        //let base_url = Url::parse(
        //    Request::builder()
        //        .uri(format!("{}/shard", BOOTSTRAP_SERVER_URL))
        //        .header(
        //            "Authorization",
        //            &format!("Bearer {}", idkey),
        //        )
        //        .method("GET")
        //        .body(())
        //        .unwrap()
        //).unwrap();
        //println!("vase_url: {:?}", base_url);

        Self {
            idkey,
            //_pd: PhantomData,
        }
    }

    pub fn listen() {}
}

mod tests {
    use crate::server_comm::ServerComm;

    #[test]
    fn test_new() {
        ServerComm::new("1234");
    }
}
