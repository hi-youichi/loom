//! Registry of active ACP transports.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::connection::{AcpConnection, ConnectionId};

#[derive(Debug, Default)]
pub struct ConnectionRegistry {
    connections: RwLock<HashMap<ConnectionId, Arc<AcpConnection>>>,
}

impl ConnectionRegistry {
    pub fn insert(&self, connection: Arc<AcpConnection>) {
        self.connections
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(connection.id.clone(), connection);
    }

    pub fn get(&self, connection_id: &str) -> Option<Arc<AcpConnection>> {
        self.connections
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(connection_id)
            .cloned()
    }

    pub fn remove(&self, connection_id: &str) -> Option<Arc<AcpConnection>> {
        self.connections
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(connection_id)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.connections
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}
