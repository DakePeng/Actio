use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

const MAX_BUFFER_BYTES: usize = 160_000;

pub struct AudioCoordinator {
    sessions: Arc<Mutex<BTreeMap<String, SessionState>>>,
}

struct SessionState {
    next_seq: i32,
    buffer: BTreeMap<i32, AudioChunkData>,
    total_bytes: usize,
}

pub struct AudioChunkData {
    pub data: Vec<u8>,
    pub timestamp_ms: i64,
    pub sequence_num: i32,
}

impl Default for AudioCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCoordinator {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub async fn create_session(&self, session_id: String) {
        self.sessions.lock().await.insert(
            session_id,
            SessionState {
                next_seq: 0,
                buffer: BTreeMap::new(),
                total_bytes: 0,
            },
        );
    }

    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.lock().await.remove(session_id);
    }

    pub async fn buffer_chunk(
        &self,
        session_id: &str,
        sequence_num: i32,
        timestamp_ms: i64,
        data: Vec<u8>,
    ) -> Vec<AudioChunkData> {
        let mut sessions = self.sessions.lock().await;
        let session = match sessions.get_mut(session_id) {
            Some(s) => s,
            None => return vec![],
        };

        let chunk_size = data.len();
        session.buffer.insert(
            sequence_num,
            AudioChunkData {
                data,
                timestamp_ms,
                sequence_num,
            },
        );
        session.total_bytes += chunk_size;

        while session.total_bytes > MAX_BUFFER_BYTES {
            if let Some((seq, chunk)) = session.buffer.pop_first() {
                session.total_bytes -= chunk.data.len();
                warn!(session_id, seq, "Dropping oldest chunk (backpressure)");
            } else {
                break;
            }
        }

        let mut ready = vec![];
        while let Some(chunk) = session.buffer.remove(&session.next_seq) {
            session.total_bytes -= chunk.data.len();
            session.next_seq += 1;
            ready.push(chunk);
        }
        ready
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ordered_chunk_delivery() {
        let coord = AudioCoordinator::new();
        coord.create_session("s1".into()).await;

        let r1 = coord.buffer_chunk("s1", 2, 1200, vec![1]).await;
        assert!(r1.is_empty());

        let r2 = coord.buffer_chunk("s1", 0, 0, vec![2]).await;
        assert_eq!(r2.len(), 1);

        let r3 = coord.buffer_chunk("s1", 1, 600, vec![3]).await;
        assert_eq!(r3.len(), 2);
    }

    #[tokio::test]
    async fn backpressure_drops_oldest() {
        let coord = AudioCoordinator::new();
        coord.create_session("s1".into()).await;

        for i in 0..100 {
            coord.buffer_chunk("s1", i, i as i64 * 600, vec![0u8; 5000]).await;
        }

        let sessions = coord.sessions.lock().await;
        let session = sessions.get("s1").unwrap();
        assert!(session.total_bytes <= MAX_BUFFER_BYTES);
    }
}
