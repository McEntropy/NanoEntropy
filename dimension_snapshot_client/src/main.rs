use std::io::Cursor;
use std::path::PathBuf;

use log::LevelFilter;
use mcprotocol::prelude::drax::nbt::{size_nbt, write_nbt};
use mcprotocol::registry::{RegistryError, UNKNOWN_VERSION};
use mcprotocol::{
    pin_fut,
    pipeline::{AsyncMinecraftProtocolPipeline, MinecraftProtocolWriter},
    prelude::drax::nbt::CompoundTag,
    protocol::{
        handshaking::sb::{Handshake, NextState},
        login::sb::LoginStart,
        play::cb::JoinGame,
    },
};
use tokio::net::TcpStream;

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

    let folder_path = std::env::args()
        .skip(1)
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./snapshots/"));
    if folder_path.exists() {
        if !folder_path.is_dir() {
            anyhow::bail!("Snapshots dir must be a directory.")
        }
    } else {
        std::fs::create_dir_all(&folder_path)?;
    };

    let versions_to_try = vec![760];

    for version in versions_to_try {
        let connection = TcpStream::connect("127.0.0.1:25566").await?;
        let (read, write) = connection.into_split();

        let handshake = Handshake {
            protocol_version: version,
            server_address: "127.0.0.1".to_string(),
            server_port: 25566,
            next_state: NextState::Login,
        };

        let login_start = LoginStart {
            name: "DockerContainer".to_string(),
            sig_data: None,
            sig_holder: None,
        };

        let mut pipeline = AsyncMinecraftProtocolPipeline::from_handshake(read, &handshake);
        pipeline.register(pin_fut!(handle_join_game));

        let mut packet_writer =
            MinecraftProtocolWriter::from_protocol_version(write, UNKNOWN_VERSION);
        let proto = handshake.protocol_version;
        packet_writer.write_packet(handshake).await?;
        packet_writer.update_protocol(proto);
        packet_writer.write_packet(login_start).await?;

        loop {
            match pipeline.execute_next_packet(&mut ()).await {
                Ok(tag) => {
                    let tag: CompoundTag = tag;
                    let mut bytes = Cursor::new(Vec::<u8>::with_capacity(size_nbt(&tag)));
                    write_nbt(&tag, &mut bytes)?;
                    let bytes = bytes.into_inner();
                    let mut file_path = folder_path.clone();
                    file_path.push(format!("{}.b.nbt", version));
                    if file_path.exists() {
                        if file_path.is_dir() {
                            anyhow::bail!("Cannot override a directory for snapshots.");
                        }
                        std::fs::remove_file(&file_path)?;
                    }
                    std::fs::write(file_path, &bytes)?;
                    break;
                }
                Err(registry_error) => {
                    match registry_error {
                        RegistryError::NoHandlerFound(packet, data) => {
                            // pass, this is okay
                            println!("Received unknown packet {:?}, ({})", packet, data.len());
                        }
                        RegistryError::DraxTransportError(err) => {
                            anyhow::bail!(err)
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

pub async fn handle_join_game(_: &mut (), join_game: JoinGame) -> CompoundTag {
    let JoinGame {
        player_id,
        hardcore,
        game_type,
        previous_game_type,
        levels,
        codec,
        dimension_type,
        dimension,
        seed,
        max_players,
        chunk_radius,
        simulation_distance,
        reduced_debug_info,
        show_death_screen,
        is_debug,
        is_flat,
        last_death_location,
    } = join_game;
    let n = JoinGame {
        player_id,
        hardcore,
        game_type,
        previous_game_type,
        levels,
        codec: CompoundTag::new(),
        dimension_type,
        dimension,
        seed,
        max_players,
        chunk_radius,
        simulation_distance,
        reduced_debug_info,
        show_death_screen,
        is_debug,
        is_flat,
        last_death_location,
    };
    println!("{:#?}", n);
    codec
}
