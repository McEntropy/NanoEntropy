use std::io::Cursor;
use std::sync::Arc;

use crate::Config;
use mcprotocol::commands::{Command, NodeStub};
use mcprotocol::pipeline::MinecraftProtocolWriter;
use mcprotocol::prelude::drax::nbt::read_nbt;
use mcprotocol::prelude::drax::nbt::CompoundTag;
use mcprotocol::prelude::drax::transport::TransportProcessorContext;
use mcprotocol::prelude::drax::VarInt;
use mcprotocol::prelude::drax::{ctg, nbt::Tag, transport::encryption::EncryptedWriter};
use mcprotocol::protocol::chunk::Chunk;
use mcprotocol::protocol::login::MojangIdentifiedKey;
use mcprotocol::protocol::play::cb::{
    DeclareCommands, JoinGame, LevelChunkWithLight, PlayerAbilities, PlayerInfo, PlayerPosition,
    PluginMessage, SetSubtitle, SetTitle, SetTitleAnimationTimes,
};
use mcprotocol::protocol::play::{
    AddPlayerEntry, GameType, LevelChunkData, LightUpdateData, PlayerAbilitiesBitMap,
    RelativeArgument,
};
use mcprotocol::protocol::GameProfile;
use mcprotocol::registry::RegistryError;
use tokio::net::tcp::OwnedWriteHalf;

pub fn dimension_from_protocol(
    protocol_version: VarInt,
) -> mcprotocol::prelude::drax::transport::Result<CompoundTag> {
    let mut buf = Cursor::new(match protocol_version {
        760 => Vec::from(*include_bytes!(
            "../dimension_snapshot_client/snapshots/760.b.nbt"
        )),
        _ => Vec::from(*include_bytes!(
            "../dimension_snapshot_client/snapshots/760.b.nbt"
        )),
    });
    read_nbt(&mut buf, 0x200000u64).map(|x| x.expect("Compound tag should exist."))
}

pub(super) async fn send_join_packets(
    writer: &mut MinecraftProtocolWriter<EncryptedWriter<OwnedWriteHalf>>,
    mojang_key: Option<MojangIdentifiedKey>,
    profile: GameProfile,
    cfg: Arc<Config>,
) -> Result<(), RegistryError> {
    let biome_tag = dimension_from_protocol(760)
        .unwrap()
        .get_tag(&"minecraft:worldgen/biome".to_string())
        .cloned()
        .unwrap();

    let join_game_tag = ctg! {
        "minecraft:dimension_type": {
            "type": "minecraft:dimension_type",
            "value": [ctg!{
                "name": "minecraft:overworld",
                "id": 0,
                "element": {
                    "natural": 1u8,
                    "coordinate_scale": 1.0f64,
                    "natural": 1u8,
                    "coordinate_scale": 1.0f64,
                    "height": 384,
                    "min_y": -64i32,
                    "piglin_safe" : 0u8,
                    "has_raids": 1u8,
                    "logical_height": 384,
                    "ultrawarm": 0u8,
                    "bed_works": 1u8,
                    "respawn_anchor_works" : 0u8,
                    "ambient_light": 0.0f32,
                    "effects" : "minecraft:overworld",
                    "has_skylight": 1u8,
                    "monster_spawn_block_light_limit": 0,
                    "infiniburn": "#minecraft:infiniburn_overworld",
                    "has_ceiling": 0u8,
                    "monster_spawn_light_level" : {
                        "type": "minecraft:uniform",
                        "value": {
                            "min_inclusive": 0,
                            "max_inclusive": 7
                        }
                    }
                }
            }]
        },
        "minecraft:worldgen/biome": biome_tag,
        "minecraft:chat_type": {
            "type": "minecraft:chat_type",
            "value": [ctg!{
                "name": "minecraft:chat",
                "id": 0,
                "element": {
                    "narration": {
                        "translation_key": "chat.type.text.narrate",
                        "parameters": (Tag::ListTag(8, vec! [
                            Tag::string_tag("sender"),
                            Tag::string_tag("content"),
                        ]))
                    },
                    "chat": {
                        "translation_key": "chat.type.text",
                        "parameters": (Tag::ListTag(8, vec! [
                            Tag::string_tag("sender"),
                            Tag::string_tag("content"),
                        ]))
                    }
                }
            }]
        }
    };

    writer
        .write_packet(&JoinGame {
            player_id: 1,
            hardcore: false,
            game_type: GameType::Adventure,
            previous_game_type: GameType::None,
            levels: vec!["minecraft:overworld".to_string()],
            codec: join_game_tag,
            dimension_type: "minecraft:overworld".to_string(),
            dimension: "minecraft:overworld".to_string(),
            seed: 0,
            max_players: 20,
            chunk_radius: 0,
            simulation_distance: 10,
            reduced_debug_info: false,
            show_death_screen: true,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
        })
        .await?;

    let mut brand_data = Cursor::new(Vec::new());
    mcprotocol::prelude::drax::extension::write_string(
        32767,
        &"Nano Entropy".to_string(),
        &mut TransportProcessorContext::new(),
        &mut brand_data,
    )?;
    if writer.protocol_version() > 340 {
        writer
            .write_packet(&PluginMessage {
                identifier: mcprotocol::protocol::constants::minecraft_channels::BRAND_CHANNEL
                    .to_string(),
                data: brand_data.into_inner(),
            })
            .await?;
    } else {
        writer
            .write_packet(&PluginMessage {
                identifier:
                    mcprotocol::protocol::constants::minecraft_channels::LEGACY_BRAND_CHANNEL
                        .to_string(),
                data: brand_data.into_inner(),
            })
            .await?;
    }

    writer
        .write_packet(&PlayerAbilities {
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
        .write_packet(&PlayerPosition {
            x: 0.0,
            y: 50.0,
            z: 0.0,
            y_rot: 0.0,
            x_rot: 0.0,
            relative_arguments: RelativeArgument::from_mask(0x08),
            id: 0,
            dismount_vehicle: false,
        })
        .await?;

    log::info!(
        "Successfully logged in player {} ({})",
        profile.name,
        profile.id
    );

    writer
        .write_packet(&PlayerInfo::AddPlayer(vec![AddPlayerEntry {
            profile: profile.clone(),
            game_type: GameType::Adventure,
            latency: 55,
            display_name: None,
            key_data: mojang_key.as_ref().cloned(),
        }]))
        .await?;

    writer
        .write_packet(&DeclareCommands {
            commands: vec![Command {
                command_flags: 0,
                children: vec![],
                redirect: None,
                node_stub: NodeStub::Root,
            }],
            root_index: 0,
        })
        .await?;

    for x in -5..5 {
        for z in -5..5 {
            let mut chunk = Chunk::new(x, z);
            chunk.rewrite_plane(1, 3).expect("Plane should rewrite.");
            chunk.rewrite_plane(2, 3).expect("Plane should rewrite.");
            chunk.rewrite_plane(3, 1).expect("Plane should rewrite.");
            chunk.rewrite_plane(4, 2).expect("Plane should rewrite.");

            chunk.set_block_id(7, 25, 8, 2).expect("Block should set");

            writer
                .write_packet(&LevelChunkWithLight {
                    chunk_data: LevelChunkData {
                        chunk,
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
        }
    }

    writer
        .write_packet(&SetTitleAnimationTimes {
            fade_in: 0,
            stay: i32::MAX,
            fade_out: 0,
        })
        .await?;
    writer
        .write_packet(&SetTitle {
            text: cfg.game.title.clone(),
        })
        .await?;
    writer
        .write_packet(&SetSubtitle {
            text: cfg.game.subtitle.clone(),
        })
        .await?;

    Ok(())
}
