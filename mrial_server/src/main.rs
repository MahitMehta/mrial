mod audio;
mod cli;
mod conn;
mod events;
mod video;

use std::env;
use base64::{engine::general_purpose::STANDARD, Engine as _};

use cli::handle_cli;
use conn::ConnectionManager;
use tokio::runtime::Runtime;
use video::VideoServerThread;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        handle_cli(&args);
        return Ok(());
    }

    let runtime = Runtime::new().expect("Failed to create tokio runtime.");
    let tokio_handle = runtime.handle().clone(); 

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    println!("Starting Mrial Server Version {}\n", VERSION);

    pretty_env_logger::init_timed();
    let conn = ConnectionManager::new(tokio_handle);

    // TODO: Temporary code for testing
    let desc_data: String = env::var("RTC_DESC").expect("RTC_DESC not set");

    let desc_data = String::from_utf8(STANDARD.decode(desc_data)?)?;

    if let Ok(web) = conn.get_web() {
        runtime.block_on(async move {
            if let Err(e) = web.initialize_client(desc_data).await {
                log::error!("Failed to initialize Web Client: {}", e);
            }
        });
    }

    let conn_clone = conn.try_clone()?;

    let mut video_server = match VideoServerThread::new(conn_clone) {
        Ok(server) => server,
        Err(e) => {
            log::error!("Failed to start Video Server: {}", e);
            return Ok(());
        }
    };

    runtime.block_on(async {
        if let Err(e) = video_server.run().await {
            log::error!("Failed to start Video Server: {}", e);
        }
    });

    Ok(())
}
