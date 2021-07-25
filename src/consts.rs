/// The maximum size of a chunk.
///
/// This should be 64 KiB as Garry's Mod will not network a Lua file larger than this.
pub const MAX_LUA_SIZE: usize = 65535;
pub(crate) const MEM_PREALLOCATE_MAX: usize = 1024 * 1024 * 1024;
pub(crate) const TERMINATOR_HACK: u8 = '|' as u8;
