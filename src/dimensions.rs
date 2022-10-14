use mcprotocol::prelude::drax::nbt::{read_nbt, CompoundTag};
use mcprotocol::prelude::drax::VarInt;

pub fn dimension_from_protocol(
    protocol_version: VarInt,
) -> mcprotocol::prelude::drax::transport::Result<CompoundTag> {
    let mut buf = std::io::Cursor::new(match protocol_version {
        760 => Vec::from(*include_bytes!(
            "../dimension_snapshot_client/snapshots/760.b.nbt"
        )),
        _ => Vec::from(*include_bytes!(
            "../dimension_snapshot_client/snapshots/760.b.nbt"
        )),
    });
    read_nbt(&mut buf, 0x200000u64).map(|x| x.expect("Compound tag should exist."))
}
