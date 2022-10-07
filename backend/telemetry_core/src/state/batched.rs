use super::{state::State as OrdinaryState, AddNodeResult, NodeAddedToChain, NodeId};
use crate::{
    aggregator::ConnId,
    feed_message::{self, FeedMessageSerializer, FeedMessageWriter},
};
use bimap::BiMap;
use common::{
    internal_messages::{MuteReason, ShardNodeId},
    node_message,
    node_types::{BlockHash, NodeDetails},
};
use std::collections::HashMap;

/// Structure with accumulated chain updates
#[derive(Default, Clone)]
struct ChainUpdates {
    /// Chain feed with all its updates
    feed: FeedMessageSerializer,
    /// Current node count
    node_count: usize,
    has_chain_label_changed: bool,
    /// Current chain label
    chain_label: Box<str>,
}

/// Wrapper which batches updates to state.
#[derive(Clone)]
pub struct State {
    // Previous state (which is read only)
    prev: OrdinaryState,
    // Next state (which is write only)
    next: OrdinaryState,
    /// Accumulated updates for each chain
    chains: HashMap<BlockHash, ChainUpdates>,
    /// We maintain a mapping between NodeId and ConnId+LocalId, so that we know
    /// which messages are about which nodes.
    node_ids: BiMap<NodeId, (ConnId, ShardNodeId)>,
}

impl State {
    pub fn new(denylist: impl IntoIterator<Item = String>, max_third_party_nodes: usize) -> Self {
        Self {
            prev: OrdinaryState::new([], max_third_party_nodes),
            next: OrdinaryState::new(denylist, max_third_party_nodes),
            chains: HashMap::new(),
            node_ids: BiMap::new(),
        }
    }

    /// Drain updates for all feeds and return serializer.
    pub fn drain_updates_for_all_feeds(&mut self) -> FeedMessageSerializer {
        let mut feed = FeedMessageSerializer::new();
        for (genesis_hash, chain_updates) in &mut self.chains {
            let ChainUpdates {
                node_count,
                has_chain_label_changed,
                chain_label,
                ..
            } = chain_updates;

            if *has_chain_label_changed {
                feed.push(feed_message::RemovedChain(*genesis_hash));
                *has_chain_label_changed = false;
            }

            feed.push(feed_message::AddedChain(
                chain_label,
                *genesis_hash,
                *node_count,
            ));
        }
        feed
    }

    /// Method which would return updates for each chain with its genesis hash
    pub fn drain_chain_updates(
        &'_ mut self,
    ) -> impl Iterator<Item = (BlockHash, FeedMessageSerializer)> + '_ {
        self.prev.clone_from(&self.next);
        self.chains
            .iter_mut()
            .map(|(genesis_hash, updates)| (*genesis_hash, std::mem::take(&mut updates.feed)))
    }

    pub fn add_node(
        &mut self,
        genesis_hash: BlockHash,
        shard_conn_id: ConnId,
        local_id: ShardNodeId,
        node: NodeDetails,
    ) -> Result<NodeId, MuteReason> {
        let NodeAddedToChain {
            id: node_id,
            new_chain_label,
            node,
            chain_node_count,
            has_chain_label_changed,
            ..
        } = match self.next.add_node(genesis_hash, node) {
            AddNodeResult::NodeAddedToChain(details) => details,
            AddNodeResult::ChainOverQuota => return Err(MuteReason::Overquota),
            AddNodeResult::ChainOnDenyList => return Err(MuteReason::ChainNotAllowed),
        };

        // Record ID <-> (shardId,localId) for future messages:
        self.node_ids.insert(node_id, (shard_conn_id, local_id));

        let updates = self.chains.entry(genesis_hash).or_default();

        // Tell chain subscribers about the node we've just added:
        updates.feed.push(feed_message::AddedNode(
            node_id.get_chain_node_id().into(),
            node,
        ));

        updates.has_chain_label_changed = has_chain_label_changed;
        updates.node_count = chain_node_count;
        updates.chain_label = new_chain_label.to_owned().into_boxed_str();

        Ok(node_id)
    }

    pub fn update_node(
        &mut self,
        shard_conn_id: ConnId,
        local_id: ShardNodeId,
        payload: node_message::Payload,
    ) {
        let node_id = match self.node_ids.get_by_right(&(shard_conn_id, local_id)) {
            Some(id) => *id,
            None => {
                log::error!(
                    "Cannot find ID for node with shard/connectionId of {:?}/{:?}",
                    shard_conn_id,
                    local_id
                );
                return;
            }
        };
        if let Some(chain) = self.next.get_chain_by_node_id(node_id) {
            let updates = self.chains.entry(chain.genesis_hash()).or_default();
            self.next.update_node(node_id, payload, &mut updates.feed);
        }
    }
}
