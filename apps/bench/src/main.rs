use std::env;

mod fam_bench;
mod pass_bench;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1].eq("pass") {
        println!("Running pass_bench");
        pass_bench::run().await;
    } else if args.len() > 1 && args[1].eq("fam") {
        println!("Running fam_bench");
        fam_bench::run().await;
    } else {
        println!("Running nothing");
    }
}
