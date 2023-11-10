/// Packet constants

/// Uuid length
pub const UUID_LEN: usize   = 16;

/// Maximum packet size (to avoid fragmentation).
/// Very conservative: This is TCP/IPv6 default MSS - one could add a few bytes for the smaller UDP hdr (or even do PMTU)
pub const MSS: usize = 1220;

/// Packet type: New rpc call
pub const PKT_CALL: u8 = 0;
/// Packet type: Reply for RPC call
pub const PKT_RPLY: u8 = 1;

