use crate::codec::*;
use crate::message::*;
use crate::util::*;
use crate::{
    util, MultiPeer, NonBlockingSend, SocketBackend, SocketFrontend, SocketType, ZmqResult,
};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::channel::{mpsc, oneshot};
use std::net::SocketAddr;
use std::sync::Arc;

pub(crate) struct Subscriber {
    pub(crate) subscriptions: Vec<Vec<u8>>,
    pub(crate) send_queue: mpsc::Sender<Message>,
    pub(crate) _io_close_handle: futures::channel::oneshot::Sender<bool>,
}

pub(crate) struct PubSocketBackend {
    subscribers: DashMap<PeerIdentity, Subscriber>,
}

#[async_trait]
impl SocketBackend for PubSocketBackend {
    async fn message_received(&self, peer_id: &PeerIdentity, message: Message) {
        let message = match message {
            Message::Message(m) => m,
            _ => return,
        };
        let data: Vec<u8> = message.into();
        if data.len() < 1 {
            return;
        }
        match data[0] {
            1 => {
                // Subscribe
                self.subscribers
                    .get_mut(&peer_id)
                    .unwrap()
                    .subscriptions
                    .push(Vec::from(&data[1..]));
            }
            0 => {
                // Unsubscribe
                let mut del_index = None;
                let sub = Vec::from(&data[1..]);
                for (idx, subscription) in self
                    .subscribers
                    .get(&peer_id)
                    .unwrap()
                    .subscriptions
                    .iter()
                    .enumerate()
                {
                    if &sub == subscription {
                        del_index = Some(idx);
                        break;
                    }
                }
                if let Some(index) = del_index {
                    self.subscribers
                        .get_mut(&peer_id)
                        .unwrap()
                        .subscriptions
                        .remove(index);
                }
            }
            _ => return,
        }
    }

    fn socket_type(&self) -> SocketType {
        SocketType::PUB
    }

    fn shutdown(&self) {
        self.subscribers.clear();
    }
}

#[async_trait]
impl MultiPeer for PubSocketBackend {
    async fn peer_connected(
        &self,
        peer_id: &PeerIdentity,
    ) -> (mpsc::Receiver<Message>, oneshot::Receiver<bool>) {
        let default_queue_size = 100;
        let (out_queue, out_queue_receiver) = mpsc::channel(default_queue_size);
        let (stop_handle, stop_callback) = oneshot::channel::<bool>();

        self.subscribers.insert(
            peer_id.clone(),
            Subscriber {
                subscriptions: vec![],
                send_queue: out_queue,
                _io_close_handle: stop_handle,
            },
        );
        (out_queue_receiver, stop_callback)
    }

    async fn peer_disconnected(&self, peer_id: &PeerIdentity) {
        println!("Client disconnected {:?}", peer_id);
        self.subscribers.remove(peer_id);
    }
}

pub struct PubSocket {
    pub(crate) backend: Arc<PubSocketBackend>,
    _accept_close_handle: Option<oneshot::Sender<bool>>,
}

impl Drop for PubSocket {
    fn drop(&mut self) {
        self.backend.shutdown();
    }
}

impl NonBlockingSend for PubSocket {
    fn send(&mut self, message: ZmqMessage) -> ZmqResult<()> {
        for mut subscriber in self.backend.subscribers.iter_mut() {
            for sub_filter in &subscriber.subscriptions {
                if sub_filter.as_slice() == &message.data[0..sub_filter.len()] {
                    let _res = subscriber
                        .send_queue
                        .try_send(Message::Message(message.clone()));
                    // TODO handle result
                    break;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl SocketFrontend for PubSocket {
    fn new() -> Self {
        Self {
            backend: Arc::new(PubSocketBackend {
                subscribers: DashMap::new(),
            }),
            _accept_close_handle: None,
        }
    }

    async fn bind(&mut self, endpoint: &str) -> ZmqResult<()> {
        let stop_handle = util::start_accepting_connections(endpoint, self.backend.clone()).await?;
        self._accept_close_handle = Some(stop_handle);
        Ok(())
    }

    async fn connect(&mut self, endpoint: &str) -> ZmqResult<()> {
        let addr = endpoint.parse::<SocketAddr>()?;
        let raw_socket = tokio::net::TcpStream::connect(addr).await?;
        util::peer_connected(raw_socket, self.backend.clone()).await;
        Ok(())
    }
}
