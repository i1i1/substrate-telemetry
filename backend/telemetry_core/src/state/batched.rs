use crate::feed_message::FeedMessageSerializer;

use super::state::State as OrdinaryState;
use common::node_types::BlockHash;
use std::collections::HashMap;

/// Structure with accumulated chain updates
#[derive(Default)]
pub struct ChainUpdates {
    feed: FeedMessageSerializer,
}

/// Wrapper which batches updates to state.
pub struct State {
    // Previous state (which is read only)
    prev: OrdinaryState,
    // Next state (which is write only)
    next: OrdinaryState,
    /// Accumulated updates for each chain
    chains: HashMap<BlockHash, ChainUpdates>,
}

impl State {
    pub fn new(denylist: impl IntoIterator<Item = String>, max_third_party_nodes: usize) -> Self {
        Self {
            prev: OrdinaryState::new([], max_third_party_nodes),
            next: OrdinaryState::new(denylist, max_third_party_nodes),
            chains: HashMap::new(),
        }
    }
}
