use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const LEASE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub struct ClientRadioRegistry {
    leases: Arc<Mutex<HashMap<i64, ClientRadioLease>>>,
}

struct ClientRadioLease {
    client_instance_id: String,
    lease_id: String,
    deadline: Instant,
}

impl ClientRadioRegistry {
    pub async fn register(&self, radio_id: i64, client_instance_id: String) -> String {
        let mut leases = self.leases.lock().await;
        leases.retain(|_, lease| lease.client_instance_id != client_instance_id);
        let lease_id = Uuid::new_v4().to_string();
        leases.insert(
            radio_id,
            ClientRadioLease {
                client_instance_id,
                lease_id: lease_id.clone(),
                deadline: Instant::now() + LEASE_TIMEOUT,
            },
        );
        lease_id
    }

    pub async fn heartbeat(
        &self,
        radio_id: i64,
        client_instance_id: &str,
        lease_id: &str,
    ) -> Result<(), LeaseError> {
        let mut leases = self.leases.lock().await;
        let now = Instant::now();
        let Some(lease) = leases.get_mut(&radio_id) else {
            return Err(LeaseError::Stale);
        };
        if lease.deadline <= now {
            leases.remove(&radio_id);
            return Err(LeaseError::Stale);
        }
        if lease.client_instance_id != client_instance_id || lease.lease_id != lease_id {
            return Err(LeaseError::Stale);
        }
        lease.deadline = now + LEASE_TIMEOUT;
        Ok(())
    }

    pub async fn offline(
        &self,
        radio_id: i64,
        client_instance_id: &str,
        lease_id: &str,
    ) -> Result<(), LeaseError> {
        let mut leases = self.leases.lock().await;
        let now = Instant::now();
        let Some(lease) = leases.get(&radio_id) else {
            return Err(LeaseError::Stale);
        };
        if lease.deadline <= now {
            leases.remove(&radio_id);
            return Err(LeaseError::Stale);
        }
        if lease.client_instance_id != client_instance_id || lease.lease_id != lease_id {
            return Err(LeaseError::Stale);
        }
        leases.remove(&radio_id);
        Ok(())
    }

    pub async fn is_online(&self, radio_id: i64) -> bool {
        let mut leases = self.leases.lock().await;
        let now = Instant::now();
        match leases.get(&radio_id) {
            Some(lease) if lease.deadline > now => true,
            Some(_) => {
                leases.remove(&radio_id);
                false
            }
            None => false,
        }
    }

    pub async fn expire(&self) -> Vec<i64> {
        let mut leases = self.leases.lock().await;
        let now = Instant::now();
        let expired = leases
            .iter()
            .filter_map(|(radio_id, lease)| (lease.deadline <= now).then_some(*radio_id))
            .collect::<Vec<_>>();
        for radio_id in &expired {
            leases.remove(radio_id);
        }
        expired
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    Stale,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn heartbeat_refreshes_and_expiry_removes_a_lease() {
        let registry = ClientRadioRegistry::default();
        let lease_id = registry.register(7, "client-a".to_string()).await;

        tokio::time::advance(Duration::from_secs(20)).await;
        registry.heartbeat(7, "client-a", &lease_id).await.unwrap();
        tokio::time::advance(Duration::from_secs(29)).await;
        assert!(registry.is_online(7).await);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(registry.expire().await, vec![7]);
        assert!(!registry.is_online(7).await);
    }

    #[tokio::test]
    async fn replacement_invalidates_the_previous_lease() {
        let registry = ClientRadioRegistry::default();
        let old_lease = registry.register(7, "client-a".to_string()).await;
        let new_lease = registry.register(7, "client-a".to_string()).await;

        assert_ne!(old_lease, new_lease);
        assert_eq!(
            registry.heartbeat(7, "client-a", &old_lease).await,
            Err(LeaseError::Stale)
        );
        assert!(registry.heartbeat(7, "client-a", &new_lease).await.is_ok());
    }
}
