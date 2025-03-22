use std::sync::{self, Arc};

use bytes::Bytes;
use kanal::{bounded_async, unbounded, AsyncReceiver, AsyncSender, Sender};
use log::{debug, error};

use mrial_proto::{deploy::{Broadcaster, PacketDeployer}, EPacketType};
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

use super::BroadcastTaskError;

#[derive(Clone)]
struct WebClient {
    peer_connection: Arc<RTCPeerConnection>,
    data_channel: Arc<RTCDataChannel>,
}

type BroadcastPayload = (EPacketType, Vec<u8>);

pub struct WebConnection {
    // TODO: Change this to a hashmap with the key as some client ID
    clients: Arc<RwLock<Vec<WebClient>>>,

    broadcast_sender: Sender<BroadcastPayload>,
    broadcast_receiver: AsyncReceiver<BroadcastPayload>,
    broadcast_task: Arc<sync::RwLock<Option<JoinHandle<()>>>>,

    input_sender: AsyncSender<Bytes>,
    input_receiver: AsyncReceiver<Bytes>,
}

const STUN_SERVER: &str = "stun:stun.l.google.com:19302";

struct WebVideoBroadcaster {
    clients: Arc<RwLock<Vec<WebClient>>>,
}

impl Broadcaster for WebVideoBroadcaster {
    async fn broadcast(&self, bytes: &[u8]) {
        let clients = self.clients.read().await;

        for client in clients.iter(){
            let bytes = Bytes::from(bytes.to_owned());
            if let Err(e) = client.data_channel.send(&bytes).await {
                error!("Failed to Broadcast Audio to Client (Disconnecting): {e}");

                let _ = client.data_channel.close().await;
                let _ = client.peer_connection.close().await;
            }
        }
    }
}

struct WebAudioBroadcaster {
    clients: Arc<RwLock<Vec<WebClient>>>
}

impl Broadcaster for WebAudioBroadcaster {
    async fn broadcast(&self, bytes: &[u8]) {
        let clients = self.clients.read().await;

        for client in clients.iter(){
            // if client.muted {
            //     continue;
            // }

            let bytes = Bytes::from(bytes.to_owned());
            if let Err(e) = client.data_channel.send(&bytes).await {
                error!("Failed to Broadcast Audio to Client (Disconnecting): {e}");

                let _ = client.data_channel.close().await;
                let _ = client.peer_connection.close().await;
            }
        }
    }
}

struct WebBroadcastTask {
    receiver: AsyncReceiver<BroadcastPayload>,

    video_deployer: PacketDeployer,
    audio_deployer: PacketDeployer,
    video_broadcaster: WebVideoBroadcaster,
    audio_broadcaster: WebAudioBroadcaster,
}

impl WebBroadcastTask {
    #[inline]
    async fn broadcast(&mut self, payload: BroadcastPayload) {
        let (packet_type, buf) = payload;

        match packet_type {
             EPacketType::NAL => {
                 self.video_deployer.slice_and_send(&buf, &self.video_broadcaster).await;
             }
             EPacketType::AudioPCM => {
                 self.audio_deployer.slice_and_send(&buf, &self.audio_broadcaster).await;
             }
             _ => {
                 error!("Unsupported Packet Type (Dropping): {:?}", packet_type);
             }
        }
    }

    async fn broadcast_loop(&mut self) {
        while let Ok(payload) = self.receiver.recv().await {
            self.broadcast(payload).await;
        }
    }

    pub fn run(
        tokio_handle: Handle,
        clients: Arc<RwLock<Vec<WebClient>>>,
        receiver: AsyncReceiver<BroadcastPayload>,
    ) -> JoinHandle<()> {
        tokio_handle.spawn(async move {
            let mut thread = Self { 
                audio_deployer: PacketDeployer::new(EPacketType::AudioPCM, false),
                video_deployer: PacketDeployer::new(EPacketType::NAL, false),
                video_broadcaster: WebVideoBroadcaster { clients: clients.clone() },
                audio_broadcaster: WebAudioBroadcaster { clients },
                receiver 
            };

            thread.broadcast_loop().await;
        })
    }
}

const MAX_INPUT_BUFFER_SIZE: usize = 100;

impl WebConnection {
    pub fn new() -> Self {
        let (broadcast_sender, broadcast_receiver) = unbounded::<BroadcastPayload>();
        let (input_sender, input_receiver) = bounded_async::<Bytes>(MAX_INPUT_BUFFER_SIZE);

        Self {
            broadcast_task: Arc::new(sync::RwLock::new(None)),
            broadcast_sender,
            broadcast_receiver: broadcast_receiver.as_async().clone(),
            clients: Arc::new(RwLock::new(vec![])),
            input_sender,
            input_receiver,
        }
    }

    /// This function starts the broadcast spawns tokio async task
    /// that listens for web broadcast messages and sends them to all.
    /// Note: This function should be called from the main thread from inside the video server.
    pub fn start_broadcast_async_task(&self) {
        if let Ok(mut thread) = self.broadcast_task.write() {
            if thread.is_none() {
                *thread = Some(WebBroadcastTask::run(
                    Handle::current(),
                    self.clients.clone(),
                    self.broadcast_receiver.clone(),
                ));
            }
        }
    }

    #[inline]
    pub fn receiver(&self) -> AsyncReceiver<Bytes> {
        self.input_receiver.clone()
    }

    #[inline]
    pub fn broadcast_frame(&self, packet_type: EPacketType, buf: &[u8]) -> Result<(), BroadcastTaskError> {
        let bytes = buf.to_owned();

        if let Ok(task) = self.broadcast_task.read() {
            if task.is_none() {
                return Err(BroadcastTaskError::TaskNotRunning);
            }
        }

        if let Err(e) = self.broadcast_sender.send((packet_type, bytes)) {
            return Err(BroadcastTaskError::TransferFailed(e.to_string()));
        }

        Ok(())
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
        let clients = self.clients.clone();
        let input_sender = self.input_sender.clone();
        // Register data channel creation handling
        peer_connection.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
            let dc_label = data_channel.label().to_owned();
            let dc_id = data_channel.id();
            debug!("New DataChannel {dc_label} {dc_id}");

            let peer_connection_clone = peer_connection_clone.clone();
            let clients_clone = clients.clone();
            let input_sender = input_sender.clone();

            // Register channel opening handling
            Box::pin(async move {
                let data_channel_clone = Arc::clone(&data_channel);

                let dc_label2 = dc_label.clone();
                let dc_id2 = dc_id;
                data_channel.on_close(Box::new(move || {
                    debug!("Data channel closed");
                    Box::pin(async {})
                }));

                data_channel.on_open(Box::new(move || {
                    debug!("Data channel '{dc_label2}'-'{dc_id2}' open.");

                    Box::pin(async move {
                        let mut clients = clients_clone.write().await;

                        clients.push(WebClient {
                            peer_connection: peer_connection_clone,
                            data_channel: data_channel_clone,
                        });
                    })
                }));

                data_channel.on_message(Box::new(move |event: DataChannelMessage| {
                    let input_sender = input_sender.clone();

                    Box::pin(async move {
                        if let Err(e) = input_sender.send(event.data).await {
                            error!("Failed to send event to input channel: {e}");
                        }
                    })
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
}

impl Clone for WebConnection {
    fn clone(&self) -> Self {
        Self {
            clients: self.clients.clone(),
            broadcast_sender: self.broadcast_sender.clone(),
            broadcast_receiver: self.broadcast_receiver.clone(),
            broadcast_task: self.broadcast_task.clone(),
            input_sender: self.input_sender.clone(),
            input_receiver: self.input_receiver.clone(),
        }
    }
}
