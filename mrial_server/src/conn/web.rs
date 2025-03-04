use std::sync::{self, Arc};

use bytes::Bytes;
use kanal::{unbounded, AsyncReceiver, Sender};
use tokio::{runtime::Handle, sync::RwLock, task::JoinHandle};
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

    broadcast_sender: Sender<Bytes>,
    broadcast_receiver: AsyncReceiver<Bytes>,
    broadcast_thread: Arc<sync::RwLock<Option<JoinHandle<()>>>>,
}

const STUN_SERVER: &str = "stun:stun.l.google.com:19302";

struct WebBroadcastThread {
    clients: Arc<RwLock<Vec<WebClient>>>,
    receiver: AsyncReceiver<Bytes>,
}

impl WebBroadcastThread {
    #[inline]
    async fn broadcast(&self, data: Bytes) {
        // TODO: enable muting client functionalities 
        // TODO: if the packet is an audio packet.

        for client in self.clients.read().await.iter() {
            if let Err(e) = client.data_channel.send(&data).await {
                println!("Failed to send packet to client: {e}");
                
                let _ = client.data_channel.close().await;
                let _ = client.peer_connection.close().await;
            }
        }
    }

    async fn broadcast_loop(&self) {
        loop {
            if let Ok(data) = self.receiver.recv().await {
                self.broadcast(data).await;
            }
        }
    }

    pub fn run(
        tokio_handle: Handle, 
        clients: Arc<RwLock<Vec<WebClient>>>, 
        receiver: AsyncReceiver<Bytes>
    ) -> JoinHandle<()> {
        tokio_handle.spawn(async move {
            let thread = Self {
                clients,
                receiver,
            };

            thread.broadcast_loop().await;
        })
    }
}

impl WebConnection {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded::<Bytes>();

        Self {
            broadcast_thread: Arc::new(sync::RwLock::new(None)),
            broadcast_sender: sender,
            broadcast_receiver: receiver.as_async().clone(),
            clients: Arc::new(RwLock::new(vec![])),
        }
    }

    #[inline]
    pub fn broadcast(&self, data: Bytes) {
        if let Ok(thread) = self.broadcast_thread.read() {
            if thread.is_none() {
                if let Ok(mut thread) = self.broadcast_thread.write() {
                    *thread = Some(WebBroadcastThread::run(
                        Handle::current(),
                        self.clients.clone(),
                        self.broadcast_receiver.clone(),
                    ));   
                }   
            }
        }

        if let Err(e) = self.broadcast_sender.send(data) {
            println!("Failed to broadcast data: {e}");
        }
    }

    pub async fn initialize_client(
        self,
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


                    Box::pin(async move {
                        let mut clients =  clients_clone.write().await;
                        
                        clients.push(WebClient {
                            peer_connection: peer_connection_clone,
                            data_channel: data_channel_clone,
                        });
                    })
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

    #[inline]
    pub async fn filter_clients(&self) {
        self.clients.write().await.retain(|client| {
            client.peer_connection.connection_state() == RTCPeerConnectionState::Connected
        });
    }

    #[inline]
    pub async fn has_clients(&self) -> bool {
        self.clients.read().await.iter().any(|client| {
            client.peer_connection.connection_state() == RTCPeerConnectionState::Connected
        })
    }

    #[inline]
    pub fn has_clients_blocking(&self) -> bool {
        self.clients.blocking_read().iter().any(|client| {
            client.peer_connection.connection_state() == RTCPeerConnectionState::Connected
        })
    }
}

impl Clone for WebConnection {
    fn clone(&self) -> Self {
        Self {
            clients: self.clients.clone(),
            broadcast_sender: self.broadcast_sender.clone(),
            broadcast_receiver: self.broadcast_receiver.clone(),
            broadcast_thread: self.broadcast_thread.clone()
        }
    }
}
