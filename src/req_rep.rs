use async_trait::async_trait;
use futures_util::sink::SinkExt;
use tokio::net::TcpStream;
use tokio::stream::StreamExt;
use tokio_util::codec::Framed;

use crate::codec::*;
use crate::error::*;
use crate::util::raw_connect;
use crate::*;
use crate::{Socket, SocketType, ZmqResult};
use bytes::BytesMut;

pub struct ReqSocket {
    pub(crate) _inner: Framed<TcpStream, ZmqCodec>,
}

#[async_trait]
impl Socket for ReqSocket {
    async fn send(&mut self, data: Vec<u8>) -> ZmqResult<()> {
        let mut f_data = BytesMut::new();
        f_data.extend_from_slice(data.as_ref());
        let frames = vec![
            ZmqMessage {
                data: BytesMut::new().freeze(),
                more: true,
            }, // delimiter frame
            ZmqMessage {
                data: f_data.freeze(),
                more: false,
            },
        ];
        self._inner.send(Message::MultipartMessage(frames)).await
    }

    async fn recv(&mut self) -> ZmqResult<Vec<u8>> {
        {
            let delimeter: Option<ZmqResult<Message>> = self._inner.next().await;
            let delim = match delimeter {
                Some(Ok(Message::Message(m))) => m,
                Some(Ok(_)) => return Err(ZmqError::Other("Wrong message type received")),
                Some(Err(e)) => return Err(e),
                None => return Err(ZmqError::NoMessage),
            };
            assert!(delim.data.is_empty() && delim.more); // Drop delimeter frame
        }
        let message: Option<ZmqResult<Message>> = self._inner.next().await;
        match message {
            Some(Ok(Message::Message(m))) => Ok(m.data.to_vec()),
            Some(Ok(_)) => Err(ZmqError::Other("Wrong message type received")),
            Some(Err(e)) => Err(e),
            None => Err(ZmqError::NoMessage),
        }
    }
}

impl ReqSocket {
    pub async fn connect(endpoint: &str) -> ZmqResult<Self> {
        let raw_socket = raw_connect(SocketType::REQ, endpoint).await?;
        Ok(Self { _inner: raw_socket })
    }
}

pub(crate) struct RepSocketServer {
    pub(crate) _inner: TcpListener,
}

pub struct RepSocket {
    pub(crate) _inner: Framed<TcpStream, ZmqCodec>,
}

#[async_trait]
impl Socket for RepSocket {
    async fn send(&mut self, data: Vec<u8>) -> ZmqResult<()> {
        let mut f_data = BytesMut::new();
        f_data.extend_from_slice(data.as_ref());
        let frames = vec![
            ZmqMessage {
                data: BytesMut::new().freeze(),
                more: true,
            }, // delimiter frame
            ZmqMessage {
                data: f_data.freeze(),
                more: false,
            },
        ];
        self._inner.send(Message::MultipartMessage(frames)).await
    }

    async fn recv(&mut self) -> ZmqResult<Vec<u8>> {
        {
            let delimeter: Option<ZmqResult<Message>> = self._inner.next().await;
            let delim = match delimeter {
                Some(Ok(Message::Message(m))) => m,
                Some(Ok(_)) => return Err(ZmqError::Other("Wrong message type received")),
                Some(Err(e)) => return Err(e),
                None => return Err(ZmqError::NoMessage),
            };
            assert!(delim.data.is_empty() && delim.more); // Drop delimeter frame
        }
        let message: Option<ZmqResult<Message>> = self._inner.next().await;
        match message {
            Some(Ok(Message::Message(m))) => Ok(m.data.to_vec()),
            Some(Ok(_)) => Err(ZmqError::Other("Wrong message type received")),
            Some(Err(e)) => Err(e),
            None => Err(ZmqError::NoMessage),
        }
    }
}

#[async_trait]
impl SocketServer for RepSocketServer {
    async fn accept(&mut self) -> ZmqResult<Box<dyn Socket>> {
        let (socket, _) = self._inner.accept().await?;
        let mut socket = Framed::new(socket, ZmqCodec::new());
        greet_exchange(&mut socket).await?;
        ready_exchange(&mut socket, SocketType::REP).await?;
        Ok(Box::new(RepSocket { _inner: socket }))
    }
}
