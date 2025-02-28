use std::sync::{Arc, RwLock};

use bytes::Bytes;
use tokio::task;
use webrtc::{
    api::APIBuilder,
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::ice_server::RTCIceServer,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
};

use super::Connection;

#[derive(Clone)]
struct WebClient {
    peer_connection: Arc<RTCPeerConnection>,
    data_channel: Arc<RTCDataChannel>,
}

pub struct WebConnection {
    // TODO: Change this to a hashmap with the key as some client ID
    clients: Arc<RwLock<Vec<WebClient>>>,
}

const STUN_SERVER: &str = "stun:stun.l.google.com:19302";

impl WebConnection {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(vec![])),
        }
    }

    pub fn broadcast(&self, data: &[u8]) {
        // Avoid copying and converting to Bytes
        let bytes = Bytes::copy_from_slice(data);

        task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(Box::pin(async move {
                if let Ok(clients) = self.clients.read() {
                    for client in clients.iter() {
                        let _ = client.data_channel.send(&bytes).await;
                    }
                }
            }))
        });
    }

    pub async fn initialize_client<'a>(
        &'a mut self,
        desc_data: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let api = APIBuilder::new().build();

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec![STUN_SERVER.to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let peer_connection = Arc::new(api.new_peer_connection(config).await?);

        peer_connection.on_peer_connection_state_change(Box::new(
            move |s: RTCPeerConnectionState| {
                println!("Peer Connection State has changed: {s}");

                if s == RTCPeerConnectionState::Failed {
                    // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                    // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                    // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                    println!("Peer Connection has gone to failed exiting");
                    // let _ = done_tx.try_send(());
                }

                Box::pin(async {})
            },
        ));

        let peer_connection_clone = peer_connection.clone();
        let clients_clone = self.clients.clone();
        // Register data channel creation handling
        peer_connection.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
            let dc_label = data_channel.label().to_owned();
            let dc_id = data_channel.id();
            println!("New DataChannel {dc_label} {dc_id}");

            let peer_connection_clone = peer_connection_clone.clone();
            let clients_clone = clients_clone.clone();

            // Register channel opening handling
            Box::pin(async move {
                let data_channel_clone = Arc::clone(&data_channel);

                let dc_label2 = dc_label.clone();
                let dc_id2 = dc_id;
                data_channel.on_close(Box::new(move || {
                    println!("Data channel closed");
                    Box::pin(async {})
                }));

                data_channel.on_open(Box::new(move || {
                    println!("Data channel '{dc_label2}'-'{dc_id2}' open.");

                    if let Ok(mut clients) = clients_clone.write() {
                        clients.push(WebClient {
                            peer_connection: peer_connection_clone,
                            data_channel: data_channel_clone,
                        });
                    }

                    Box::pin(async move {})
                }));

                // Register text message handling
                data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
                    let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
                    println!("Message from DataChannel '{dc_label}': '{msg_str}'");
                    Box::pin(async {})
                }));
            })
        }));
        let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

        // Set the remote SessionDescription
        peer_connection.set_remote_description(offer).await?;

        // Create an answer
        let answer = peer_connection.create_answer(None).await?;

        // Create channel that is blocked until ICE Gathering is complete
        let mut gather_complete = peer_connection.gathering_complete_promise().await;

        // Sets the LocalDescription, and starts our UDP listeners
        peer_connection.set_local_description(answer).await?;

        // Block until ICE Gathering is complete, disabling trickle ICE
        // we do this because we only can exchange one signaling message
        // in a production application you should exchange ICE Candidates via OnICECandidate
        let _ = gather_complete.recv().await;

        // Output the answer in base64 so we can paste it in browser
        if let Some(local_desc) = peer_connection.local_description().await {
            let json_str = serde_json::to_string(&local_desc)?;
            println!("{json_str}");
        } else {
            println!("generate local_description failed!");
        }

        Ok(())
    }
}

impl Connection for WebConnection {
    fn filter_clients(&self) {
        // Filter clients
    }

    fn has_clients(&self) -> bool {
        // TODO: Make this more sophisticated by checking if the clients are still connected

        if let Ok(clients) = self.clients.read() {
            return !clients.is_empty();
        }

        false
    }
}

impl Clone for WebConnection {
    fn clone(&self) -> Self {
        Self {
            clients: self.clients.clone(),
        }
    }
}
