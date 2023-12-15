[![Build status](https://img.shields.io/github/actions/workflow/status/randombtree/rpcudp-rs/main.yml)](https://github.com/randombtree/rpcudp-rs/actions)

# rpcudp-rs - Simple bare-bones P2P RPC over UDP

Simple unreliable async RPC over UDP. RPC calls are not timed, so its up to the caller to time-out if the receiver doesn't answer.

Supports both async-std and tokio runtimes as features.

## Status

In development, some corner cases still not handled optimally.

## Usage
```
use rpcudp_rs::compat::net::SocketAddr;

use rpcudp_rs::{RpcServer, rpc};

/// State object for receiving RPC calls
struct MyService {
    greeting: String,
}

impl MyService {
    fn new(greeting: String) -> MyService {
        MyService { greeting }
    }
}

// Install RPC hooks to MyService using the rpc-macro.
rpc! {
    MyService {
        async fn greet(&self, name: String) -> String {
            format!("{} {}!", self.greeting, name).into()
        }

		async fn get_address(&self, context: RpcContext) -> SocketAddr {
		    context.source
		}
    }
}

async fn async_main() {
    let peer1_addr: SocketAddr = "127.0.0.1:30000".parse().unwrap();
    let peer2_addr: SocketAddr = "127.0.0.1:30001".parse().unwrap();

    // Create two peers that can do RPC to each others
    let peer1 = RpcServer::bind(peer1_addr,
                                MyService::new("Hello".into())).await.unwrap();
    let peer2 = RpcServer::bind(peer2_addr,
                                MyService::new("Hola".into())).await.unwrap();

    // Call RPC methods - note the peer address added and Result returned.
    assert!("Hola Isabel!" == peer1.greet(peer2_addr, "Isabel".into()).await.unwrap());
    assert!("Hello George!" == peer2.greet(peer1_addr, "George".into()).await.unwrap());

	// Retrieve caller address
	assert!(peer1_addr == peer1.get_address(peer2_addr).await.unwrap());
}

fn main() {
    rpcudp_rs::compat::task::block_on(async { async_main().await; });
}
```

Peer address is also available inside the RPC method under `context.source`.
