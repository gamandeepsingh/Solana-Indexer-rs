use std::env;

pub struct Config {
    pub grpc_endpoint: String,
    pub database_url: String,
    pub x_token: Option<String>,
    pub console_log: bool,
    pub bench_log: String,
    pub api_port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        let console_log = env::var("CONSOLE_LOG")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        Self {
            grpc_endpoint: env::var("GRPC_ENDPOINT").expect("GRPC_ENDPOINT must be set"),
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            x_token: env::var("X_TOKEN").ok(),
            console_log,
            bench_log: env::var("BENCH_LOG").unwrap_or_else(|_| "benchmark.log".to_string()),
            api_port: env::var("API_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
        }
    }
}
