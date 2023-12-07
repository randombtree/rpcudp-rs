#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadMe;

#[cfg(all(feature = "async-std", feature = "tokio"))]
compile_error!("Choose either \"async-std\" or \"tokio\" feature, not both!");

#[cfg(not(any(feature = "async-std", feature = "tokio")))]
compile_error!("Choose either \"async-std\" or \"tokio\" feature");


// Imports for rpc macro
pub use std::collections::HashMap;
pub use std::sync::Arc;
pub use paste::paste;
pub use log::trace;
pub use bincode;

extern crate lazy_static;
pub use lazy_static::lazy_static;


pub mod error;
#[macro_use]
pub mod service;
pub mod packet;
pub mod server;
pub mod compat;

// Some imports for making the rpc-macro more readable:
pub use crate::service::*;
pub use crate::error::*;

pub use crate::server::RpcServer;
pub use crate::error::{RpcError, Result};
pub use crate::service::RpcContext;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use std::time::Duration;
    use log::trace;
    use test_log::test;

    use super::{RpcServer, rpc};
    use super::compat::{
	task::{
	    block_on,
	    yield_now,
	},
	net:: SocketAddr,
	time::timeout,
    };

    struct TestServiceInner {
	counter: u32,
    }

    #[derive(Clone)]
    struct TestService(Arc<Mutex<TestServiceInner>>);

    impl TestService {
	fn new() -> TestService {
	    TestService(Arc::new(Mutex::new(
		TestServiceInner {
		    counter: 0,
		})))
	}

	fn counter(&self) -> u32 {
	    let inner = self.0.lock().unwrap();
	    return inner.counter;
	}

	fn inc(&self, count: u32) {
	    let mut inner = self.0.lock().unwrap();
	    inner.counter += count;
	}
    }

    rpc! {
	/// Tests different kinds of RPC calls
	TestService {
	    /// Test: string string
	    async fn hello(&self, name: String) -> String {
		trace!("In hello!");
		let mut inner = self.0.lock().unwrap();
		inner.counter += 1;
		format!("Hello {} {}", name, inner.counter).into()
	    }

	    /// Test: Two params
	    async fn add(&self, a: u32, b: u32) -> u32 {
		a + b
	    }

	    /// Test: Two types
	    async fn concat(&self, a: String, b: u32) -> String {
		format!("{}{}", a, b).into()
	    }

	    /// Test: No params
	    async fn get_counter(&self) -> u32 {
		self.counter()
	    }

	    /// Test: No return
	    async fn inc_counter(&self, count: u32) {
		self.inc(count);
	    }

	    /// Test: Use context source address
	    async fn get_source(&self, context: RpcContext) -> SocketAddr {
		context.source
	    }
	}
    }

    macro_rules! timed_future {
	($expr:expr) => {
	    timeout(Duration::from_millis(5), $expr).await
		.expect("Timed out")
	}
    }


    #[test]
    fn create_service() {
	block_on(async {
	    let server_service = TestService::new();
	    let client_service = TestService::new();
	    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
	    let [client, server] = [&client_service, &server_service]
		.map(|service| async {
		    let rpc = RpcServer::bind(addr, service.clone()).await.unwrap();
		    let addr = rpc.local_addr();
		    (rpc, addr)
		});
	    let ((client, client_addr), (server, server_addr)) = futures::future::join(client, server).await;

	    // Test string -> string
	    let hello_ret = timed_future!(client.hello(server_addr, "foo".into()));
	    assert!(hello_ret.is_ok(), "RPC Call failed?");
	    let hello = hello_ret.unwrap();
	    assert!(hello == "Hello foo 1");
	    assert!(server_service.counter() == 1, "Server counter didn't increment?");

	    let add = timed_future!(client.add(server_addr, 2, 3))
		.expect("Add RPC failed");
	    assert!(add == 5);

	    // Test two params with different types
	    let concat = timed_future!(client.concat(server_addr, "foo".into(), 1))
		.expect("RPC call failed?");
	    assert!(concat == "foo1");

	    // Test no params
	    let counter = timed_future!(client.get_counter(server_addr))
		.expect("RPC call failed?");
	    assert!(counter == server_service.counter());

	    // Test no-return method
	    let old_counter = server_service.counter();
	    let ret = timed_future!(client.inc_counter(server_addr, 2))
		.expect("RPC inc_counter failed");
	    assert!(ret == ());
	    assert!(server_service.counter() == old_counter + 2);

	    // Test with context source address
	    let addr = timed_future!(client.get_source(server_addr))
		.expect("RPC get_source failed");
	    assert!(addr == client_addr);

	    drop(server);
	    drop(client);
	    yield_now().await;
	});
    }

    #[test]
    fn proxy_object_removed_on_fail() {
	block_on(async {
	    let service = TestService::new();
	    let addr: SocketAddr = "127.0.0.1:30002".parse().unwrap();
	    let blackhole: SocketAddr = "127.0.0.1:30003".parse().unwrap();
	    let service = RpcServer::bind(addr, service).await.unwrap();
	    let ret = timeout(Duration::from_millis(5),
			      service.get_counter(blackhole)).await;
	    assert!(ret.is_err());
	    drop(ret);
	    // The future should have been removed from the hashmap, thus preventing a memory leak
	    assert!(service._get_waiter_map_len() == 0, "Proxy object memory leak");
	});
    }
}
