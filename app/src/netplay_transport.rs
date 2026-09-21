//! Stream adapters keep the wire protocol shared by direct Quinn and Iroh/Noq.
use std::time::Duration;

pub(crate) enum Connection {
    Direct(quinn::Connection),
    Iroh(iroh::endpoint::Connection),
}

impl Connection {
    pub fn rtt(&self) -> Option<Duration> {
        match self {
            Self::Direct(c) => Some(c.rtt()),
            Self::Iroh(c) => c
                .paths()
                .iter()
                .find(|p| p.is_selected())
                .and_then(|p| c.rtt(p.id())),
        }
    }

    pub fn close(&self) {
        match self {
            Self::Direct(c) => c.close(0u32.into(), b"netplay stream closed"),
            Self::Iroh(c) => c.close(0u32.into(), b"netplay stream closed"),
        }
    }

    pub fn route(&self) -> Option<&'static str> {
        match self {
            Self::Direct(_) => Some("Direct IP connection"),
            Self::Iroh(c) => c.paths().iter().find(|p| p.is_selected()).map(|p| {
                if p.is_relay() {
                    "Iroh: relayed connection"
                } else {
                    "Iroh: direct connection"
                }
            }),
        }
    }
}

pub(crate) enum SendStream {
    Direct(quinn::SendStream),
    Iroh(iroh::endpoint::SendStream),
}

impl SendStream {
    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<(), String> {
        match self {
            Self::Direct(s) => s.write_all(bytes).await.map_err(|e| e.to_string()),
            Self::Iroh(s) => s.write_all(bytes).await.map_err(|e| e.to_string()),
        }
    }

    pub fn finish(&mut self) -> Result<(), String> {
        match self {
            Self::Direct(s) => s.finish().map_err(|e| e.to_string()),
            Self::Iroh(s) => s.finish().map_err(|e| e.to_string()),
        }
    }

    pub async fn stopped(&mut self) -> Result<(), String> {
        match self {
            Self::Direct(s) => s.stopped().await.map(|_| ()).map_err(|e| e.to_string()),
            Self::Iroh(s) => s.stopped().await.map(|_| ()).map_err(|e| e.to_string()),
        }
    }
}

pub(crate) enum RecvStream {
    Direct(quinn::RecvStream),
    Iroh(iroh::endpoint::RecvStream),
}

impl RecvStream {
    pub async fn read_exact(&mut self, bytes: &mut [u8]) -> Result<(), String> {
        match self {
            Self::Direct(s) => s.read_exact(bytes).await.map_err(|e| e.to_string()),
            Self::Iroh(s) => s.read_exact(bytes).await.map_err(|e| e.to_string()),
        }
    }
}
