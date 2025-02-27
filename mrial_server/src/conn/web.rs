use std::{io::Read, sync::Arc};

use bytes::Bytes;
use webrtc::{
    api::APIBuilder, data_channel::{data_channel_init::RTCDataChannelInit, data_channel_message::DataChannelMessage, RTCDataChannel}, ice_transport::ice_server::RTCIceServer, peer_connection::{configuration::RTCConfiguration, RTCPeerConnection}
};

use super::Connection;

#[derive(Clone)]
struct WebClient {
    peer_connection: Arc<RTCPeerConnection>,
    data_channel: Arc<RTCDataChannel>
}

pub struct WebConnection {
    // TODO: Change this to a hashmap with the key as some client ID
    clients: Vec<WebClient>,
}

const STUN_SERVER: &str = "stun:stun.l.google.com:19302";

impl WebConnection {
    pub fn new() -> Self {
        Self { 
            clients: vec![] 
        }
    }

    pub fn broadcast(&self, data: &[u8]) {
        // Avoid copying and converting to Bytes
        let bytes = Bytes::copy_from_slice(data);

        for client in self.clients.iter() {    
            let _ = client.data_channel.send(&bytes);
        }
    }

    pub async fn initialize_client(
        &mut self,
    ) -> Result<(), Box::<dyn std::error::Error>> {
        let api = APIBuilder::new()
            .build();

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec![STUN_SERVER.to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let peer_connection = Arc::new(api.new_peer_connection(config).await?);

        let data_channel = peer_connection
            .create_data_channel("mrial-stream", Some(RTCDataChannelInit {
                ordered: Some(true),
                max_retransmits: Some(0),
                ..Default::default()
            }))
            .await?;

        let d_label = data_channel.label().to_owned();
        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
            println!("Message from DataChannel '{d_label}': '{msg_str}'");
            Box::pin(async {})
        }));

        let offer = peer_connection.create_offer(None).await?;
        let mut gather_complete = peer_connection.gathering_complete_promise().await;

        peer_connection.set_local_description(offer).await?;
        let _ = gather_complete.recv().await;

        if let Some(local_desc) = peer_connection.local_description().await {
            let json_str = serde_json::to_string(&local_desc)?;
            println!("Peer Description: {json_str}");
        } else {
            println!("Failed to generate local_description");
        }

        self.clients.push(WebClient {
            peer_connection,
            data_channel
        });

        Ok(())
    }
}

impl Connection for WebConnection {
    fn filter_clients(&self) {
        // Filter clients
    }

    fn has_clients(&self) -> bool {
        // Check if any clients are connected
        self.clients.len() > 0
    }
}

impl Clone for WebConnection {
    fn clone(&self) -> Self {
        Self {
            clients: self.clients.clone(),
        }
    }
}
