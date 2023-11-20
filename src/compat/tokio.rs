pub mod io {
    pub use tokio::io::{
	Result,
    };
}

pub mod net {
    pub use tokio::net::{
	ToSocketAddrs,
	UdpSocket,
    };
    pub use std::net::SocketAddr;
}

pub mod task {
    pub use tokio::task::{
	yield_now,
	spawn,
    };

    use tokio::runtime::Runtime;
    use std::future::Future;
    /// Emulate async-std block_on behaviour
    /// block_on is only used in tests, so abort:ing on failure should be a-ok
    /// (can't cfg this out due to doctest!)
    pub fn block_on<F: Future>(f: F) -> F::Output {
	let rt  = Runtime::new().unwrap();
	rt.block_on(f)
    }
}

pub mod time {
    pub use tokio::time::timeout;
}
