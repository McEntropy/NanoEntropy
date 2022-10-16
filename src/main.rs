use log::LevelFilter;
use mcprotocol::auth::mojang::AuthenticatedClient;
use mcprotocol::pin_fut;
use mcprotocol::pipeline::MinecraftProtocolWriter;
use mcprotocol::prelude::AsyncWrite;
use mcprotocol::protocol::handshaking::sb::Handshake;
use mcprotocol::protocol::login::cb::LoginSuccess;
use mcprotocol::protocol::play::cb::*;
use mcprotocol::protocol::status::cb::StatusResponsePlayers;
use mcprotocol::registry::RegistryError;
use mcprotocol::server_loop::{BaseConfiguration, IncomingAuthenticationOption, ServerLoop};
use mcprotocol::status::StatusBuilder;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::interval;

mod join;

#[derive(serde_derive::Deserialize, Debug)]
struct ListPlayerInfo {
    max_players: i32,
    online_players: i32,
}

const fn default_log_level() -> LevelFilter {
    LevelFilter::Info
}

#[derive(serde_derive::Deserialize, Debug, Clone)]
struct GameCfg {
    title: mcprotocol::chat::Chat,
    subtitle: mcprotocol::chat::Chat,
}

#[derive(serde_derive::Deserialize, Debug)]
struct Config {
    motd: mcprotocol::chat::Chat,
    player_info: ListPlayerInfo,
    #[serde(default = "default_log_level")]
    filter_level: LevelFilter,
    #[serde(skip)]
    favicon: Option<String>,
    game: GameCfg,
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let mut reader = Cursor::new(std::fs::read("./config.json")?);
    let mut config: Config = serde_json::from_reader(&mut reader)?;

    let path = Path::new("./server-icon.png");
    if path.exists() {
        let base_64 = image_base64::to_base64(path.to_str().unwrap());
        config.favicon = Some(base_64);
    }

    let config = Arc::new(config);

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
        .level(config.filter_level)
        .chain(std::io::stdout())
        .apply()?;

    let listener = TcpListener::bind("0.0.0.0:25565").await?;

    log::info!("Listener bound to 0.0.0.0:25565");

    let server_loop_cfg = config.clone();
    let server_loop = Arc::new(ServerLoop::new(
        BaseConfiguration {
            auth_option: IncomingAuthenticationOption::MOJANG,
            compression_threshold: 1024,
            force_key_authentication: true,
            auth_url: None,
        },
        pin_fut!(client_acceptor),
        move |h| Box::pin(status_responder(server_loop_cfg.clone(), h)),
    ));

    log::info!("Nano Entropy completed boot, can now accept clients.");

    loop {
        let (stream, _) = listener.accept().await?;
        let loop_clone = server_loop.clone();
        let cfg_clone = config.clone();
        tokio::spawn(async move {
            let (read, write) = stream.into_split();
            if let Err(registry_error) =
            ServerLoop::accept_client(loop_clone, InitialClientContext {
                cfg: cfg_clone,
            }, read, write).await
            {
                if !matches!(
                    registry_error,
                    RegistryError::DraxTransportError(
                        mcprotocol::prelude::drax::transport::Error::EOF
                    )
                ) {
                    log::warn!(
                        "Registry error encountered when accepting client: {}",
                        registry_error
                    );
                }
            }
        });
    }
}

struct InitialClientContext {
    cfg: Arc<Config>,
}

async fn client_acceptor(
    ctx: InitialClientContext,
    rw: AuthenticatedClient<OwnedReadHalf, OwnedWriteHalf>,
) -> Result<(), RegistryError> {
    let AuthenticatedClient {
        read_write: (mut read, mut writer),
        profile,
        mojang_key,
        ..
    } = rw;

    writer.write_packet(LoginSuccess::from(&profile)).await?;

    join::send_join_packets(&mut writer, mojang_key, profile, ctx.cfg.clone()).await?;

    let ping_writer = broadcast_pings(writer);
    let client_read: JoinHandle<Result<(), RegistryError>> = tokio::spawn(async move {
        loop {
            match read.execute_next_packet(&mut ()).await {
                Ok(_) => {}
                Err(err) => match err {
                    RegistryError::NoHandlerFound(_, _) => {}
                    RegistryError::DraxTransportError(err) => {
                        if matches!(err, mcprotocol::prelude::drax::transport::Error::EOF) {
                            return Ok(());
                        }
                        return Err(RegistryError::DraxTransportError(err));
                    }
                },
            }
        }
    });

    tokio::select! {
        _ = ping_writer => {
            log::debug!("Ping writer finished in select.");
        }
        _ = client_read => {
            log::debug!("Client read finished in select.");
        }
    }

    Ok(())
}

async fn status_responder(cfg: Arc<Config>, _: Handshake) -> StatusBuilder {
    StatusBuilder {
        players: StatusResponsePlayers {
            max: cfg.player_info.max_players,
            online: cfg.player_info.online_players,
            sample: vec![],
        },
        description: cfg.motd.clone(),
        favicon: cfg.favicon.as_ref().cloned(),
    }
}

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
