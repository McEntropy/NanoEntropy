use log::LevelFilter;
use mcprotocol::auth::mojang::AuthenticatedClient;
use mcprotocol::chat::Chat;
use mcprotocol::commands::{Command, NodeStub};
use mcprotocol::pin_fut;
use mcprotocol::protocol::chunk::Chunk;
use mcprotocol::protocol::handshaking::sb::Handshake;
use mcprotocol::protocol::login::cb::LoginSuccess;
use mcprotocol::protocol::play::cb::*;
use mcprotocol::protocol::play::RelativeArgument;
use mcprotocol::protocol::status::cb::StatusResponsePlayers;
use mcprotocol::registry::{ProtocolVersionKey, RegistryError, UNKNOWN_VERSION};
use mcprotocol::server_loop::{BaseConfiguration, IncomingAuthenticationOption, ServerLoop};
use mcprotocol::status::StatusBuilder;
use std::sync::Arc;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

mod dimensions;
mod keep_alive_broadcaster;

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
        .level(LevelFilter::Info)
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

struct InitialClientContext {}

async fn client_acceptor(
    _: InitialClientContext,
    rw: AuthenticatedClient<OwnedReadHalf, OwnedWriteHalf>,
) -> Result<(), RegistryError> {
    let AuthenticatedClient {
        read_write: (mut read, mut writer),
        profile,
        mojang_key,
        ..
    } = rw;

    let proto = read
        .retrieve_data::<ProtocolVersionKey>()
        .cloned()
        .unwrap_or(UNKNOWN_VERSION);

    writer.write_packet(LoginSuccess::from(&profile)).await?;

    writer
        .write_packet(JoinGame {
            player_id: 1,
            hardcore: false,
            game_type: GameType::Adventure,
            previous_game_type: GameType::None,
            levels: vec![
                "minecraft:overworld".to_string(),
                "minecraft:the_nether".to_string(),
                "minecraft:the_end".to_string(),
            ],
            codec: dimensions::dimension_from_protocol(proto)?,
            dimension_type: "minecraft:overworld".to_string(),
            dimension: "minecraft:overworld".to_string(),
            seed: 0,
            max_players: 20,
            chunk_radius: 0,
            simulation_distance: 0,
            reduced_debug_info: false,
            show_death_screen: true,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
        })
        .await?;

    writer
        .write_packet(PlayerAbilities {
            player_abilities_map: PlayerAbilitiesBitMap {
                invulnerable: true,
                flying: true,
                can_fly: true,
                instant_build: false,
            },
            flying_speed: 0.1,
            fov_modifier: 0.1,
        })
        .await?;

    writer
        .write_packet(PlayerPosition {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            y_rot: 0.0,
            x_rot: 0.0,
            relative_arguments: RelativeArgument::from_mask(0x08),
            id: 0,
            dismount_vehicle: false,
        })
        .await?;

    writer
        .write_packet(PlayerInfo::AddPlayer(vec![AddPlayerEntry {
            profile: profile.clone(),
            game_type: GameType::Adventure,
            latency: 55,
            display_name: None,
            key_data: mojang_key.as_ref().cloned(),
        }]))
        .await?;

    writer
        .write_packet(DeclareCommands {
            commands: vec![Command {
                command_flags: 0,
                children: vec![],
                redirect: None,
                node_stub: NodeStub::Root,
            }],
            root_index: 0,
        })
        .await?;

    writer
        .write_packet(LevelChunkWithLight {
            chunk_data: LevelChunkData {
                chunk: Chunk::new(0, 0),
                block_entities: vec![],
            },
            light_data: LightUpdateData {
                trust_edges: true,
                sky_y_mask: vec![],
                block_y_mask: vec![],
                empty_sky_y_mask: vec![],
                empty_block_y_mask: vec![],
                sky_updates: vec![vec![]; 2048],
                block_updates: vec![vec![]; 2048],
            },
        })
        .await?;

    let ping_writer = keep_alive_broadcaster::broadcast_pings(writer);
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
            return Ok(());
        }
        _ = client_read => {
            log::debug!("Client read finished in select.");
            return Ok(());
        }
    };
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
