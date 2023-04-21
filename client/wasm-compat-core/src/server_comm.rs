use crate::core::CoreClient;
use std::marker::PhantomData;
//use std::sync::Arc;
//use url::Url;
//use reqwest::{Response, Result};

//const BOOTSTRAP_SERVER_URL: &'static str = "http://localhost:8081";

pub struct ServerComm<'a, C: CoreClient> {
    //base_url: Url,
    idkey: &'a str,
    //client: reqwest::Client,
    _pd: PhantomData<C>,
}

impl<'a, C: CoreClient> ServerComm<'a, C> {
    pub fn new(
        //ip_arg: Option<&'a str>,
        //port_arg: Option<&'a str>,
        idkey: &'a str,
        //core_option: Option<Arc<Core<C>>>,
    ) -> Self {
        //let client = reqwest::Client::new();
        //let base_url = Url::parse(
        //&client
        //        .get(format!("{}/shard", BOOTSTRAP_SERVER_URL))
        //        .header(
        //            "Authorization",
        //            &format!("Bearer {}", &idkey),
        //        )
        //        .send()
        //        .await
        //        .expect("Failed to contact the bootstrap server shard")
        //        .text()
        //        .await
        //        .expect("Failed to retrieve response from the bootstrap server
        // shard")
        //).expect("Failed to construct home-shard base url from response");
        Self {
            //base_url,
            idkey,
            //client,
            _pd: PhantomData,
        }
    }
}
