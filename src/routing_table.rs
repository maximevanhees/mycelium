use ip_network_table_deps_treebitmap::IpLookupTable;
use log::warn;

use crate::{
    metric::Metric, peer::Peer, router_id::RouterId, sequence_number::SeqNo,
    source_table::SourceKey, subnet::Subnet,
};
use core::fmt;
use std::{
    cmp::Ordering,
    net::{IpAddr, Ipv6Addr},
};

#[derive(Debug, Clone, PartialEq)]
pub struct RouteKey {
    subnet: Subnet,
    neighbor: Peer,
}

impl Eq for RouteKey {}

impl PartialOrd for RouteKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RouteKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.subnet.cmp(&other.subnet) {
            Ordering::Equal => self.neighbor.overlay_ip().cmp(&other.neighbor.overlay_ip()),
            ord => ord,
        }
    }
}

impl fmt::Display for RouteKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "{} via {}",
            self.subnet,
            self.neighbor.underlay_ip()
        ))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteEntry {
    source: SourceKey,
    neighbor: Peer,
    metric: Metric,
    seqno: SeqNo,
    selected: bool,
}

impl RouteKey {
    /// Create a new `RouteKey` with the given values.
    #[inline]
    pub const fn new(subnet: Subnet, neighbor: Peer) -> Self {
        Self { subnet, neighbor }
    }

    /// Returns the [`Subnet`] associated with this `RouteKey`.
    #[inline]
    pub const fn subnet(&self) -> Subnet {
        self.subnet
    }
}

impl RouteEntry {
    /// Create a new `RouteEntry`.
    pub const fn new(
        source: SourceKey,
        neighbor: Peer,
        metric: Metric,
        seqno: SeqNo,
        selected: bool,
    ) -> Self {
        Self {
            source,
            neighbor,
            metric,
            seqno,
            selected,
        }
    }

    /// Returns the [`SourceKey`] associated with this `RouteEntry`.
    pub const fn source(&self) -> SourceKey {
        self.source
    }

    /// Returns the [`SeqNo`] associated with this `RouteEntry`.
    pub const fn seqno(&self) -> SeqNo {
        self.seqno
    }

    /// Returns the metric associated with this `RouteEntry`.
    pub const fn metric(&self) -> Metric {
        self.metric
    }

    /// Return the (neighbour)[`Peer`] associated with this `RouteEntry`.
    pub fn neighbour(&self) -> &Peer {
        &self.neighbor
    }

    /// Indicates this `RouteEntry` is the selected route for the destination.
    pub const fn selected(&self) -> bool {
        self.selected
    }

    /// Updates the metric of this `RouteEntry` to the given value.
    pub fn update_metric(&mut self, metric: Metric) {
        self.metric = metric;
    }

    /// Updates the seqno of this `RouteEntry` to the given value.
    pub fn update_seqno(&mut self, seqno: SeqNo) {
        self.seqno = seqno;
    }

    /// Updates the source [`RouterId`] of this `RouteEntry` to the given value.
    pub fn update_router_id(&mut self, router_id: RouterId) {
        self.source.set_router_id(router_id);
    }

    /// Sets whether or not this `RouteEntry` is the selected route for the associated [`Peer`].
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected
    }
}

/// The `RoutingTable` contains all known subnets, and the associated [`RouteEntry`]'s.
pub struct RoutingTable {
    table: IpLookupTable<Ipv6Addr, Vec<RouteEntry>>,
}

impl RoutingTable {
    /// Create a new, empty `RoutingTable`.
    pub fn new() -> Self {
        Self {
            table: IpLookupTable::new(),
        }
    }

    /// Get a reference to the [`RouteEntry`] associated with the [`RouteKey`] if one is present in
    /// the table.
    pub fn get(&self, key: &RouteKey) -> Option<&RouteEntry> {
        let addr = match key.subnet.address() {
            IpAddr::V6(addr) => addr,
            _ => return None,
        };

        self.table
            .exact_match(addr, key.subnet.prefix_len() as u32)?
            .iter()
            .find(|entry| entry.neighbor == key.neighbor)
    }

    /// Insert a new [`RouteEntry`] in the table. If there is already an entry for the
    /// [`RouteKey`], the existing entry is removed.
    ///
    /// Currenly only IPv6 is supported, attempting to insert an IPv4 network does nothing.
    pub fn insert(&mut self, key: RouteKey, entry: RouteEntry) {
        // We make sure that the selected route has index 0 in the entry list (if there is one).
        // This effectively makes the lookup of a selected route O(1), whereas a lookup of a non
        // selected route is O(n), n being the amount of Peers (as this is the differentiating part
        // in a key for a subnet).
        let addr = match key.subnet.network() {
            IpAddr::V6(addr) => addr,
            _ => return,
        };
        let selected = entry.selected;
        match self
            .table
            .exact_match_mut(addr, key.subnet.prefix_len() as u32)
        {
            Some(entries) => {
                let new_elem_idx = if let Some(idx) = entries
                    .iter()
                    .position(|entry| entry.neighbor == key.neighbor)
                {
                    // Overwrite entry if one exists for the key
                    entries[idx] = entry;
                    idx
                } else {
                    entries.push(entry);
                    entries.len() - 1
                };
                // If the inserted entry is selected, swap it to index 0 so it is at the start of
                // the list.
                if selected {
                    entries.swap(0, new_elem_idx);
                }
            }
            None => {
                self.table
                    .insert(addr, key.subnet.prefix_len() as u32, vec![entry]);
            }
        };
    }

    /// Make sure there is no [`RouteEntry`] in the table for a given [`RouteKey`]. If an entry
    /// existed prior to calling this, it is returned.
    ///
    /// Currenly only IPv6 is supported, attempting to remove an IPv4 network does nothing.
    pub fn remove(&mut self, key: &RouteKey) -> Option<RouteEntry> {
        let addr = match key.subnet.network() {
            IpAddr::V6(addr) => addr,
            _ => return None,
        };
        match self
            .table
            .exact_match_mut(addr, key.subnet.prefix_len() as u32)
        {
            Some(entries) => {
                let elem = entries
                    .iter()
                    .position(|entry| entry.neighbor == key.neighbor)
                    .map(|idx| entries.swap_remove(idx));
                // Remove the table entry entirely if the vec is empty
                // NOTE: we don't care if val is some, we only care that no empty set is left
                // behind.
                if entries.is_empty() {
                    self.table.remove(addr, key.subnet.prefix_len() as u32);
                }
                elem
            }
            None => None,
        }
    }

    /// Create an iterator over all key value pairs in the table.
    // TODO: remove this?
    pub fn iter(&self) -> impl Iterator<Item = (RouteKey, &'_ RouteEntry)> {
        self.table.iter().flat_map(|(addr, prefix, entries)| {
            entries.iter().map(move |value| {
                (
                    RouteKey::new(
                        Subnet::new(addr.into(), prefix as u8)
                            .expect("Only proper subnets are inserted in the table; qed"),
                        value.neighbor.clone(),
                    ),
                    value,
                )
            })
        })
    }

    /// Remove all [`RouteKey`] and [`RouteEntry`] pairs where the [`RouteEntry`]'s neighbour value
    /// is the given [`Peer`].
    pub fn remove_peer(&mut self, peer: Peer) {
        for (_, _, entries) in self.table.iter_mut() {
            entries.retain(|entry| entry.neighbor != peer);
        }
    }

    /// Look up a selected route for an [`IpAddr`] in the `RoutingTable`.
    ///
    /// Currently only IPv6 is supported, looking up an IPv4 address always returns [`Option::None`].
    /// In the event where 2 distinc routes are inserted with selected set to true, the entry with
    /// the minimum `Metric` is selected. Note that it is an error to have 2 such entries, and this
    /// might result in a panic later.
    pub fn lookup_selected(&self, ip: IpAddr) -> Option<RouteEntry> {
        let addr = match ip {
            IpAddr::V6(addr) => addr,
            _ => return None,
        };
        let entries = self.table.longest_match(addr)?.2;
        if entries.is_empty() {
            // This is a logic error in our code, but don't crash as it is recoverable.
            warn!("Empty route entry list for {ip}, this is a bug");
            return None;
        }
        if entries[0].selected {
            Some(entries[0].clone())
        } else {
            None
        }
    }

    /// Get a copy of all fallback route entries for an [`IpAddr`] in the `RoutingTable`.
    ///
    /// Currently only IPv6 is supported, looking up an IPv4 address always returns an empty
    /// [`Vec`].
    pub fn lookup_fallbacks(&self, ip: IpAddr) -> Vec<RouteEntry> {
        let addr = match ip {
            IpAddr::V6(addr) => addr,
            _ => return Vec::new(),
        };
        self.table
            .longest_match(addr)
            .map(|(_, _, entries)| entries.as_slice())
            .unwrap_or(&[])
            .iter()
            .filter(|entry| !entry.selected)
            .cloned()
            .collect()
    }
}

impl Clone for RoutingTable {
    fn clone(&self) -> Self {
        let mut new = RoutingTable::new();
        for (rk, rv) in self.iter() {
            new.insert(rk, rv.clone());
        }
        new
    }
}
