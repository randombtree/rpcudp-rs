use std::collections::HashMap;
use std::future::Future;
use std::marker::Send;
use std::pin::Pin;
use std::sync::Arc;

use async_std::net::SocketAddr;

use log::trace;

use crate::error::*;
use crate::packet::*;


/// Context for incoming RPC call.
#[allow(unused)]
pub struct RpcContext {
    /// Where did this call originate from (allegedly, note address spoofing possibility)
    source: SocketAddr,
}

impl RpcContext {
    pub(crate) fn new(source: SocketAddr) -> RpcContext {
	RpcContext { source }
    }
}


/// Service wrapper trait between server and actual dispatch to RPC method
pub trait RpcService {
    fn handle(self: Arc<Self>, context: RpcContext, input: &[u8], output: Vec<u8>) -> impl std::future::Future<Output = RpcResult> + Send;
}


// Due to constraints in Rust, e.g. missing async fn-type, lifetimes of fn-type and sized dyn traits; need to pass the output Vec around :(
// When Rust gets one of those features we could start using the Write trait for output!
pub type RpcHandlerFunc<T> = fn(Arc<T>, RpcContext, &[u8], Vec<u8>) -> Result<Pin<Box<dyn Future<Output = RpcResult> + Send>>>;
pub type RpcHash<T> = HashMap<&'static str, RpcHandlerFunc<T>>;
use std::str::from_utf8;
/// Decode and call RPC function
pub async fn handle_rpc_call<T>(state: Arc<T>, context: RpcContext, input: &[u8], mut output: Vec<u8>, map: &RpcHash<T>) -> RpcResult {

    const FUNC_START: usize = UUID_LEN + 1;
    // Can't be shorter than uuid + lengt + name
    let len = input.len();
    if len < (UUID_LEN + 2) {
	trace!("Too short packet");
	return Err(RpcError::InvalidHeader);
    }
    let uuid = &input[0..UUID_LEN];
    let func_len = usize::from(input[UUID_LEN]);
    if func_len == 0 || len < FUNC_START + func_len - 1 {
	trace!("Too short packet for function name");
	return Err(RpcError::InvalidHeader);
    }
    let func = &input[FUNC_START.. FUNC_START + func_len];
    let func = from_utf8(func)?;
    trace!("RPC to {}", func);
    let func = map.get(func)
	.ok_or(RpcError::MethodNotFound)?;
    for b in uuid {
	output.push(*b);
    }
    let ret = func(state, context, &input[(FUNC_START + func_len)..], output)?.await?;
    trace!("RPC End");
    Ok(ret)
}


pub fn append_uuid(buf: &mut Vec<u8>) {
    for _ in 0..UUID_LEN {
	buf.push(rand::random());
    }
}


/// Create RPC service
/// Credits: Ideas from rsrpc service macro
#[macro_export] macro_rules! rpc {
    (
	$(#[$srvattr:meta])*
	$rpc_name:ident {
	    $(
		$(#[$fnattr:meta])*
		async fn $fn_name:ident(& $self_:ident $(, $arg:ident : $in:ty)*) $(-> $ret:ty)? $b:block
	    )*
	}
    ) => {

	$crate::paste! {
	    // Hide RPC methods into own namespace
	    trait [<$rpc_name Trait>] {
		$(
		    async fn $fn_name(& $self_, context: $crate::service::RpcContext $(, $arg: $in)*) -> rpc!(@ret $($ret)?);
		)*
	    }

	    // Proxy interface for rpc calls
	    trait [<$rpc_name Interface>] {
		$(
		    async fn $fn_name(& $self_, dst: SocketAddr $(, $arg: $in)*) -> $crate::Result<rpc!(@ret $($ret)?)>;
		)*
	    }

	    $(#[$srvattr])*
	    #[allow(unused)]
	    impl [<$rpc_name Trait>] for $rpc_name {
		$(
		    // Apply function attributes to the RPC body.
		    $(#[$fnattr])*
		    async fn $fn_name(& $self_, context: $crate::service::RpcContext $(, $arg: $in)*) -> rpc!(@ret $($ret)?) $b
		)*
	    }

	    // I would love to use compile-time initialized phf for this but, alas, the lazy
	    // macro evaluation in rust declarative macros kills phf_map.. so early init it is.
	    $crate::lazy_static! {
		// handler functions mapped by rpc function name.
		static ref [<_ $rpc_name:snake:upper >]: $crate::service::RpcHash<$rpc_name> = {
		    let mut m = $crate::HashMap::<&'static str, $crate::service::RpcHandlerFunc<$rpc_name>>::new();
		    $(
			m.insert(stringify!($fn_name), |this, context, input, mut output: Vec<u8>| {
			    $crate::trace!("RPC Handler {}", stringify!($fn_name));
			    let this = this.clone();
			    let ( $($arg,)*) : ($($in,)*) = bincode::deserialize(input)
				.or(Err($crate::RpcError::ArgumentDecode))?;
			    let func = || async move {
				$crate::trace!("Start RPC -> {}", stringify!($fn_name));
				// Make sure we use the trait method!
				let ret = [<$rpc_name Trait>]::$fn_name(&*this, context, $($arg,)*).await;
				$crate::trace!("End RPC   <- {}",  stringify!($fn_name));
				//let writer = &output as &dyn Write;
				$crate::bincode::serialize_into(&mut output, &ret).unwrap();
				Ok(output)
			    };
			    Ok(Box::pin(func()))
			});
		    )*
		    m
		};
	    }

	    // Incoming RPC call wrapper
	    impl $crate::service::RpcService for $rpc_name {
		fn handle(self: $crate::Arc<Self>, context: $crate::service::RpcContext, input: &[u8], output: Vec<u8>) -> impl std::future::Future<Output = $crate::error::RpcResult> + Send {
		    $crate::service::handle_rpc_call(self, context, input, output, &[<_ $rpc_name:snake:upper >])
		}
	    }

	    // Outgoing "proxy"

	    impl [<$rpc_name Interface>] for RpcServer<$rpc_name> {
		$(
		    // Also apply attributes to the proxy method, so that call validation can happen
		    // both on the proxy (caller) side and at the target
		    $(#[$fnattr])*
		    async fn $fn_name(& $self_, dst: SocketAddr $(, $arg: $in)*) -> $crate::error::Result<rpc!(@ret $($ret)?)> {
			let name = stringify!($fn_name);
			$crate::trace!("Call proxy method {}", name);
			// TODO: Enforce capacity
			let mut buf = Vec::with_capacity($crate::packet::MSS);
			buf.push($crate::packet::PKT_CALL);
			$crate::service::append_uuid(&mut buf);
			let name_bytes = name.as_bytes();
			// Set call name
			buf.push(name_bytes.len() as u8);
			for b in name_bytes.iter() {
			    buf.push(*b);
			}
			// serialize call args
			$(
			    bincode::serialize_into(&mut buf, & $arg).unwrap();
			)*
			let ret = $self_.call(dst, buf).await.await;
			Ok(bincode::deserialize(ret.as_slice()).unwrap())
		    }
		)*
	    }
	}
    };
    // Fix up/"de-sugar" methods without return type to returning the actual ()
    (@ret $ret:ty) => { $ret };
    (@ret) => { () };
}

