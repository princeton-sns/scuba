use server_comm::*;
use futures::TryStreamExt;

fn main() {
    let mut sc = ServerComm::new(None, None, "abcde");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            while let Ok(e) = sc.try_next().await {
                println!("{:?}", e);
            }
        })
}
