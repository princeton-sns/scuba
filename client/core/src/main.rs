use noise_core::core::Core;

fn main() {
  let mut core = Core::new(None, None, false);

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
