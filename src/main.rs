use log::LevelFilter;
use mcprotocol::auth::mojang::AuthenticatedClient;
use mcprotocol::chat::Chat;
use mcprotocol::pin_fut;
use mcprotocol::protocol::handshaking::sb::Handshake;
use mcprotocol::protocol::login::cb::{Disconnect, LoginSuccess};
use mcprotocol::protocol::status::cb::StatusResponsePlayers;
use mcprotocol::registry::RegistryError;
use mcprotocol::server_loop::{BaseConfiguration, IncomingAuthenticationOption, ServerLoop};
use mcprotocol::status::StatusBuilder;
use std::sync::Arc;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpListener;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{} [{}/{}]: {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(LevelFilter::Trace)
        .chain(std::io::stdout())
        .apply()?;

    let listener = TcpListener::bind("127.0.0.1:25565").await?;

    log::info!("Listener bound to 127.0.0.1:25565");

    let server_loop = Arc::new(ServerLoop::new(
        BaseConfiguration {
            auth_option: IncomingAuthenticationOption::MOJANG,
            compression_threshold: 1024,
            force_key_authentication: true,
            auth_url: None,
        },
        pin_fut!(client_acceptor),
        |h| Box::pin(status_responder(h)),
    ));

    log::info!("Nano Entropy completed boot, can now accept clients.");

    loop {
        let (stream, _) = listener.accept().await?;
        let loop_clone = server_loop.clone();
        tokio::spawn(async move {
            let (read, write) = stream.into_split();
            if let Err(registry_error) =
                ServerLoop::accept_client(loop_clone, InitialClientContext {}, read, write).await
            {
                log::warn!(
                    "Registry error encountered when accepting client: {}",
                    registry_error
                );
            }
        });
    }
}

struct InitialClientContext {}

async fn client_acceptor(
    _: InitialClientContext,
    rw: AuthenticatedClient<OwnedReadHalf, OwnedWriteHalf>,
) -> Result<(), RegistryError> {
    let AuthenticatedClient {
        read_write: (_, mut writer),
        profile,
        key: _,
    } = rw;
    writer.write_packet(LoginSuccess::from(&profile)).await?;

    

    Ok(())
}

async fn status_responder(_: Handshake) -> StatusBuilder {
    return StatusBuilder {
        players: StatusResponsePlayers {
            max: 2,
            online: -1,
            sample: vec![],
        },
        description: Chat::literal("Nano Entropy"),
        favicon: None,
    };
}
