use std::sync::Arc;
use std::sync::Mutex;
use std::marker::{Sync, Send};
use std::future::Future;
use std::pin::Pin;
use std::collections::HashMap;
use std::mem::swap;
use std::task::{Waker, Context, Poll};

use futures::{
    channel::oneshot,
    future::FutureExt,
    select,
};


use async_std::net::{
    ToSocketAddrs,
    SocketAddr,
    UdpSocket,
};
use async_std::io::Result as IOResult;
use async_std::task;

use log::{debug, trace, error};

use crate::packet::*;
use crate::service::{RpcService, RpcContext};

enum RpcProxyState {
    INIT,
    PENDING(Waker),
    READY(Vec<u8>),
    FINISHED,
}

impl RpcProxyState {
    fn new() -> RpcProxyState {
	RpcProxyState::INIT
    }
}

#[allow(unused)] // TODO: server will be used, remove when done
pub(crate) struct RpcProxyInner<T: Sync> {
    state: Mutex<RpcProxyState>,
    server: Arc<RpcServerInner<T>>,
}

impl<T: Sync> RpcProxyInner<T> {
    fn new(server: Arc<RpcServerInner<T>>) -> RpcProxyInner<T> {
	RpcProxyInner {
	    state: Mutex::new(RpcProxyState::new()),
	    server,
	}
    }

    /// Set the result for the Future waiting and wake up waiter.
    fn wakeup(&self, result: Vec<u8>) {
	let mut state = self.state.lock().unwrap();
	let mut new_state = RpcProxyState::READY(result);
	swap(&mut *state, &mut new_state);
	if let RpcProxyState::PENDING(waker) = new_state {
	    waker.wake();
	}
    }
}


/// RpcProxy is where the client/caller waits for the return value from the remote server.
pub struct RpcProxy<T: Sync>(Arc<RpcProxyInner<T>>);
impl<T: Sync> RpcProxy<T> {
    fn new(server: Arc<RpcServerInner<T>>) -> RpcProxy<T> {
	RpcProxy(Arc::new(RpcProxyInner::new(server)))
    }
}


impl<T: Sync> Future for RpcProxy<T> {
    type Output = Vec<u8>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
	let mut state = self.0.state.lock().unwrap();
	use RpcProxyState::*;

	// Ugh, this got a bit ugly, fix this if a match'n'swap pattern can be found
	match *state {
	    INIT | PENDING(_) => {
		swap(&mut *state, &mut RpcProxyState::PENDING(cx.waker().clone()));
		Poll::Pending
	    },
	    FINISHED => Poll::Pending,
	    READY(_) => {

		let mut oldstate = FINISHED;
		swap(&mut *state, &mut oldstate);

		if let READY(out) = oldstate {
		    Poll::Ready(out)
		} else {
		    Poll::Pending
		}
	    },
	}

    }
}

impl<T: Sync> Drop for RpcProxy<T> {
    fn drop(&mut self) {
	// TODO: Remove RpcProxy from hashmap
	// This is nececcary if the remote doesn't send a reply
    }
}


struct RpcServerInner<T: Sync> {
    /// User supplied object for RPC calls
    user_state: Arc<T>,
    socket: Arc<UdpSocket>,
    /// Pending out-going calls, uuid -> waiter object
    pending: Mutex<HashMap<[u8;16], Arc<RpcProxyInner<T>>>>,
}

impl<T: Sync> RpcServerInner<T> {
    fn new(user_state: Arc<T>, socket: Arc<UdpSocket>) -> RpcServerInner<T> {
	RpcServerInner {
	    user_state,
	    socket,
	    pending: Mutex::new(HashMap::new()),
	}
    }
}


pub struct RpcServer<T: RpcService + Sync>
{
    inner: Arc<RpcServerInner<T>>,
    /// When quit_marker is dropped, the receive end (i.e. the server) will get woken up and can quit
    #[allow(unused)]
    quit_marker: oneshot::Sender<()>,
}



impl<T: RpcService + Sync + Send +'static> RpcServer<T>
{
    pub async fn bind<A: ToSocketAddrs>(addr: A, state: T) -> IOResult<RpcServer<T>> {
	let socket = Arc::new(UdpSocket::bind(addr).await?);
	let rcv_socket = socket.clone();
	// Need a way to stop spawned reader task when server is stopped (dropped)
	let (s_quit, r_quit) = oneshot::channel::<()>();
	let user_state = Arc::new(state);
	let inner = Arc::new(RpcServerInner::new(user_state, socket));
	let server_inner = inner.clone();
	task::spawn(async move {
	    let mut r_quit = r_quit.fuse();
	    let inner = server_inner;

	    trace!("Server running");
	    loop {
		let mut buf = vec![0; MSS];

		select! {
		    _quit = r_quit => {
			debug!("Quit signalled, stopping RPC server");
			break;
		    },
		    rcv_result = rcv_socket.recv_from(&mut buf).fuse() => {
			if let Ok((size, src)) = rcv_result {
			    buf.truncate(size);
			    trace!("Incoming packet from {}", src);
			    Self::handle_packet(inner.clone(), src, buf);
			} else {
			    error!("Error receiving {}", rcv_result.unwrap_err());
			    break;
			}
		    }
		}
	    }
	    trace!("Server stopped");
	});

	Ok(RpcServer {
	    inner,
	    quit_marker: s_quit,
	})
    }

    fn handle_packet(inner: Arc<RpcServerInner<T>>, source: SocketAddr, packet: Vec<u8>) {

	match packet[0] {
	    PKT_CALL => {
		trace!("Call packet");
		let context = RpcContext::new(source);
		// Run call in detached task as to not block the server
		task::spawn(async move {
		    let mut output = Vec::with_capacity(MSS);
		    output.push(PKT_RPLY);
		    let state = inner.user_state.clone();
		    let len = packet.len();
		    match RpcService::handle(state, context, &packet[1..len], output).await {
			Ok(buf) => {
			    trace!("Sending reply");
			    match inner.socket.send_to(buf.as_slice(), source).await {
				Ok(bytes) => {
				    trace!("Sent {} bytes", bytes);
				}
				Err(e) => {
				    trace!("Error sending reply: {}", e)
				}
			    }
			},
			Err(e) => {
			    error!("Call failed: {:?}", e);
			}
		    }
		});
	    },
	    PKT_RPLY => {
		trace!("Reply packet");
		if packet.len() < UUID_LEN + 1 {
		    debug!("Short packet received! ({})", packet.len());
		    return;
		}
		let uuid: [u8; UUID_LEN] = std::array::from_fn(|i| packet[i + 1]);

		//let mut map = inner.pending.lock().unwrap();
		let mut map = inner.pending.lock().unwrap();
		let ret = map.remove(&uuid)
		    .and_then(|proxy| {
			// Ugh, not good at all.. need a buffer object :/
			let mut result = Vec::with_capacity(packet.len() - UUID_LEN - 1);
			for b in &packet[1 + UUID_LEN..] {
			    result.push(*b);
			}
			proxy.wakeup(result);
			Some(())
		    });
		if ret.is_some() {
		    trace!("Reply data relayed to caller");
		} else {
		    debug!("Spurious reply packet, couldn't find waiter");
		}

	    },
	    _ => {
		debug!("Invalid packet");
	    }
	}

    }

    /// Translate proxy call to outbound packet call
    pub async fn call(&self, dst: SocketAddr, packet: Vec<u8> ) -> RpcProxy<T> {
	trace!("Calling");
	let uuid: [u8; UUID_LEN] = std::array::from_fn(|i| packet[i + 1]);
	let proxy = RpcProxy::new(self.inner.clone());
	{
	    let mut map = self.inner.pending.lock().unwrap();
	    map.insert(uuid, proxy.0.clone());
	}
	match self.inner.socket.send_to(packet.as_slice(), dst).await {
	    Ok(_b) => (),
	    Err(e) => error!("Failed to send packet: {}", e),
	}
	proxy
    }

}