use std::time::Duration;

use tonic::transport::{Channel, ClientTlsConfig};

pub async fn connect(endpoint: &str) -> Channel {
    Channel::from_shared(endpoint.to_string())
        .expect("Invalid gRPC endpoint URL")
        .tls_config(ClientTlsConfig::new().with_native_roots())
        .expect("Failed to configure TLS")
        .http2_adaptive_window(true)
        .initial_connection_window_size(1 << 23) // 8 MB
        .initial_stream_window_size(1 << 23) // 8 MB
        .tcp_keepalive(Some(Duration::from_secs(10)))
        .connect_timeout(Duration::from_secs(15))
        .connect()
        .await
        .expect("Failed to connect to gRPC endpoint")
}
