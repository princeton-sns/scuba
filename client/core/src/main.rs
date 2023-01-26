use noise_core_lib::core::Core;

fn main() {
  let mut core = Core::new();

  tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap()
      .block_on(async {
        loop {
          core.handle_events().await;
        }
      })
}
