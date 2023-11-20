
pub use async_std::future;

pub mod io {
    pub use async_std::io::{
	Result,
    };
}

pub mod net {
    pub use async_std::net::{
	ToSocketAddrs,
	SocketAddr,
	UdpSocket,
    };
}

pub mod task {
    pub use async_std::task::{
	block_on,
	yield_now,
	spawn,
    };
}

pub mod time {
    pub use async_std::future::timeout;
}
