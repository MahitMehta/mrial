mod audio;
mod cli;
mod conn;
mod events;
mod video;

use std::env;
use base64::{engine::general_purpose::STANDARD, Engine as _};

use cli::handle_cli;
use conn::ConnectionManager;
use video::VideoServerThread;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        handle_cli(&args);
        return Ok(());
    }

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    println!("Starting Mrial Server Version {}\n", VERSION);

    pretty_env_logger::init_timed();
    let conn = ConnectionManager::new().await;

    // TODO: Temporary code for testing
    let desc_data: String = env::var("RTC").expect("RTC not set");

    let desc_data = String::from_utf8(STANDARD.decode(desc_data)?)?;

    if let Ok(web) = conn.get_web() {
        if let Err(e) = web.initialize_client(desc_data).await {
            log::error!("Failed to initialize Web Client: {}", e);
        }
    }

    let conn_clone = conn.clone();

    let mut video_server = match VideoServerThread::new(conn_clone).await {
        Ok(server) => server,
        Err(e) => {
            log::error!("Failed to start Video Server: {}", e);
            return Ok(());
        }
    };

    if let Err(e) = video_server.run().await {
        log::error!("Failed to start Video Server: {}", e);
    }

    Ok(())
}
