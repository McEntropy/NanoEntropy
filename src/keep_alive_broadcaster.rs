use std::time::UNIX_EPOCH;

use mcprotocol::protocol::play::cb::KeepAlive;
use mcprotocol::{pipeline::MinecraftProtocolWriter, prelude::AsyncWrite, registry::RegistryError};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

pub fn broadcast_pings<W: AsyncWrite + Unpin + Sized + Send + Sync + 'static>(
    mut protocol_writer: MinecraftProtocolWriter<W>,
) -> JoinHandle<Result<(), RegistryError>> {
    tokio::task::spawn(async move {
        let mut interval = interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            protocol_writer
                .write_packet(KeepAlive {
                    id: std::time::SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                })
                .await?;
        }
    })
}
