/// Errors and Result types
use std::str::Utf8Error;

use bincode;


#[derive(Debug)]
pub enum RpcError {
    /// Wrong data/length of RPC header
    InvalidHeader,
    /// Error decoding RPC method name
    InvalidMethodName,
    /// Method not found
    MethodNotFound,
    /// Arguments couldn't be decoded
    ArgumentDecode,
    /// Decoding RPC call arguments, some error happened
    InvalidArguments(bincode::Error),
}


impl From<Utf8Error> for RpcError {
    fn from(_error: Utf8Error) -> Self {
	RpcError::InvalidMethodName
    }
}

/// Standard RPC Result
pub type Result<V> = std::result::Result<V, RpcError>;

/// Internal RPC result
pub type RpcResult = std::result::Result<Vec<u8>, RpcError>;
