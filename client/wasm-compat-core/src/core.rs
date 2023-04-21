use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type SequenceNumber = u128;

#[async_trait]
pub trait CoreClient: Sync + Send + 'static {
    async fn client_callback(
        &self,
        seq: SequenceNumber,
        sender: String,
        message: String,
    );
}
pub struct Core<C: CoreClient> {
    client: RwLock<Option<Arc<C>>>,
}

impl<C: CoreClient> Core<C> {
    pub async fn new<'a>(
        ip_arg: Option<&'a str>,
        port_arg: Option<&'a str>,
        //turn_encryption_off: bool,
        client: Option<Arc<C>>,
    ) -> Arc<Core<C>> {
        // Core needs to effectively register itself as a client of
        // server_comm (no trait needed b/c only one implementation
        // will ever be used, at least at this point) - which is why
        // Core::new() should return Arc<Core<C>>

        let arc_core = Arc::new(Core {
            client: RwLock::new(client),
        });

        arc_core
    }
}
