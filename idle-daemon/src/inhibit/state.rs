// SPDX-License-Identifier: MIT

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(not(test))]
use std::time::{Duration, Instant};

use zbus::names::UniqueName;

use super::external::list_external;
use super::merge::merge_inhibitor_rows;

#[derive(Debug, Clone)]
pub struct Inhibitor {
    pub cookie: u32,
    pub application_name: String,
    pub reason: String,
    pub client: UniqueName<'static>,
}

#[derive(Debug)]
pub struct InhibitorState {
    inhibitors: Mutex<Vec<Inhibitor>>,
    last_cookie: AtomicU32,
    /// Cache of “any external block active” + last probe time.
    #[cfg(not(test))]
    logind_cache: Mutex<(bool, Instant)>,
    /// Last time we pruned D-Bus-dead unique names from local cookies.
    #[cfg(not(test))]
    prune_cache: Mutex<Instant>,
}

impl InhibitorState {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            inhibitors: Mutex::new(Vec::new()),
            last_cookie: AtomicU32::new(0),
            #[cfg(not(test))]
            logind_cache: Mutex::new((
                false,
                Instant::now()
                    .checked_sub(Duration::from_secs(5))
                    .unwrap_or_else(Instant::now),
            )),
            #[cfg(not(test))]
            prune_cache: Mutex::new(
                Instant::now()
                    .checked_sub(Duration::from_secs(5))
                    .unwrap_or_else(Instant::now),
            ),
        }
    }

    pub fn len(&self) -> usize {
        self.inhibitors
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_inhibited(&self) -> bool {
        self.maybe_prune_dead_clients();

        if !self
            .inhibitors
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .is_empty()
        {
            return true;
        }

        #[cfg(test)]
        {
            false
        }
        #[cfg(not(test))]
        {
            let mut cache = self
                .logind_cache
                .lock()
                .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
            if cache.1.elapsed() >= Duration::from_secs(2) {
                cache.0 = !list_external().is_empty();
                cache.1 = Instant::now();
            }
            cache.0
        }
    }

    /// Add a hold. Same client+app+reason is coalesced (returns existing cookie).
    pub fn add(
        &self,
        application_name: String,
        reason: String,
        client: UniqueName<'static>,
    ) -> Result<u32, &'static str> {
        if application_name.len() > 1024 || reason.len() > 1024 {
            return Err("application_name or reason exceeds maximum length of 1024 bytes");
        }

        let mut inhibitors = self
            .inhibitors
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        if let Some(existing) = inhibitors.iter().find(|entry| {
            entry.client == client
                && entry.application_name == application_name
                && entry.reason == reason
        }) {
            return Ok(existing.cookie);
        }
        let count = inhibitors
            .iter()
            .filter(|entry| entry.client == client)
            .count();
        if count >= 32 {
            return Err("too many concurrent inhibitors for this client");
        }
        let cookie = self.last_cookie.fetch_add(1, Ordering::Relaxed) + 1;
        inhibitors.push(Inhibitor {
            cookie,
            application_name,
            reason,
            client,
        });
        Ok(cookie)
    }

    /// Remove an inhibitor only when `cookie` belongs to `client`.
    pub fn remove_for_client(&self, cookie: u32, client: &UniqueName<'_>) -> bool {
        let mut inhibitors = self
            .inhibitors
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        if let Some(index) = inhibitors
            .iter()
            .position(|entry| entry.cookie == cookie && entry.client == *client)
        {
            inhibitors.remove(index);
            true
        } else {
            false
        }
    }

    pub fn remove_client(&self, client: &UniqueName<'_>) {
        let mut inhibitors = self
            .inhibitors
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        inhibitors.retain(|entry| entry.client.as_str() != client.as_str());
    }

    /// Drop holds whose D-Bus unique name is no longer on the session bus.
    pub fn prune_not_in_live_set(&self, live_unique: &HashSet<String>) -> usize {
        let mut inhibitors = self
            .inhibitors
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        let before = inhibitors.len();
        inhibitors.retain(|entry| live_unique.contains(entry.client.as_str()));
        before.saturating_sub(inhibitors.len())
    }

    fn maybe_prune_dead_clients(&self) {
        #[cfg(test)]
        {
            let _ = self;
        }
        #[cfg(not(test))]
        {
            let mut last = self
                .prune_cache
                .lock()
                .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
            if last.elapsed() < Duration::from_secs(2) {
                return;
            }
            *last = Instant::now();
            drop(last);
            if let Some(live) = session_unique_names() {
                let n = self.prune_not_in_live_set(&live);
                if n > 0 {
                    idle_log::info!("pruned {n} inhibitor(s) from departed D-Bus clients");
                }
            }
        }
    }

    /// IdleScreen cookies only (D-Bus UnInhibit targets these).
    pub fn list(&self) -> Vec<(u32, String, String)> {
        self.maybe_prune_dead_clients();
        let inhibitors = self
            .inhibitors
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        inhibitors
            .iter()
            .map(|entry| {
                (
                    entry.cookie,
                    entry.application_name.clone(),
                    entry.reason.clone(),
                )
            })
            .collect()
    }

    /// Full picture for `idlescreen inhibitors`: cookies + logind idle + MPRIS
    /// + the daemon's own battery policy (else status.inhibited is unexplained).
    pub fn list_all(&self) -> Vec<(u32, String, String)> {
        let mut rows = merge_inhibitor_rows(self.list(), &list_external());
        if crate::daemon::battery::is_on_battery() {
            rows.push((
                0,
                "battery".into(),
                "on battery power — savers suppressed".into(),
            ));
        }
        rows
    }
}

/// Live unique connection names on the session bus (`:1.N`).
#[cfg(not(test))]
fn session_unique_names() -> Option<HashSet<String>> {
    use super::zbus_helper::safe_zbus_blocking;
    safe_zbus_blocking(|| {
        let conn = zbus::blocking::Connection::session().ok()?;
        let proxy = zbus::blocking::fdo::DBusProxy::new(&conn).ok()?;
        let names = proxy.list_names().ok()?;
        Some(
            names
                .into_iter()
                .map(|n| n.to_string())
                .filter(|n| n.starts_with(':'))
                .collect(),
        )
    })
    .flatten()
}
