//! Execution context.
use std::{
    any::Any,
    collections::btree_map::{BTreeMap, Entry},
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use io_context::Context as IoContext;
use slog::{self, o};

use oasis_core_runtime::{
    common::{logger::get_logger, namespace::Namespace},
    consensus,
    consensus::roothash,
    protocol::HostInfo,
    storage::mkvs,
    transaction::context::Context as RuntimeContext,
};

use crate::{
    crypto::random::Rng,
    event::{Event, EventTag, EventTags},
    keymanager::KeyManager,
    module::MethodHandler as _,
    modules::core::Error,
    runtime,
    storage::{self, NestedStore, Store},
    types::{address::Address, message::MessageEventHookInvocation, transaction},
};

/// Transaction execution mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    ExecuteTx,
    CheckTx,
    SimulateTx,
    PreScheduleTx,
}

const MODE_CHECK_TX: &str = "check_tx";
const MODE_EXECUTE_TX: &str = "execute_tx";
const MODE_SIMULATE_TX: &str = "simulate_tx";
const MODE_PRE_SCHEDULE_TX: &str = "pre_schedule_tx";

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.into())
    }
}

impl From<&Mode> for &'static str {
    fn from(m: &Mode) -> Self {
        match m {
            Mode::CheckTx => MODE_CHECK_TX,
            Mode::ExecuteTx => MODE_EXECUTE_TX,
            Mode::SimulateTx => MODE_SIMULATE_TX,
            Mode::PreScheduleTx => MODE_PRE_SCHEDULE_TX,
        }
    }
}

/// Local configuration key the value of which determines whether expensive queries should be
/// allowed or not, and also whether smart contracts should be simulated for `core.EstimateGas`.
/// DEPRECATED and superseded by LOCAL_CONFIG_ESTIMATE_GAS_BY_SIMULATING_CONTRACTS and LOCAL_CONFIG_ALLOWED_QUERIES.
const LOCAL_CONFIG_ALLOW_EXPENSIVE_QUERIES: &str = "allow_expensive_queries";
/// Local configuration key the value of which determines whether smart contracts should
/// be simulated when estimating gas in `core.EstimateGas`.
const LOCAL_CONFIG_ESTIMATE_GAS_BY_SIMULATING_CONTRACTS: &str =
    "estimate_gas_by_simulating_contracts";
/// Local configuration key the value of which determines the set of allowed queries.
const LOCAL_CONFIG_ALLOWED_QUERIES: &str = "allowed_queries";
/// Special key inside the `allowed_queries` list; represents the set of all queries.
const LOCAL_CONFIG_ALLOWED_QUERIES_ALL: &str = "all";
/// Special key inside the `allowed_queries` list; represents the set of all queries
/// that are tagged `expensive`.
const LOCAL_CONFIG_ALLOWED_QUERIES_ALL_EXPENSIVE: &str = "all_expensive";

/// Runtime SDK context.
pub trait Context {
    /// Runtime that the context is being invoked in.
    type Runtime: runtime::Runtime;
    /// Runtime state output type.
    type Store: Store;

    /// Returns a logger.
    fn get_logger(&self, module: &'static str) -> slog::Logger;

    /// Context mode.
    fn mode(&self) -> Mode;

    /// Whether the transaction is just being checked for validity.
    fn is_check_only(&self) -> bool {
        self.mode() == Mode::CheckTx || self.mode() == Mode::PreScheduleTx
    }

    /// Whether the transaction is just being checked for validity before being scheduled.
    fn is_pre_schedule(&self) -> bool {
        self.mode() == Mode::PreScheduleTx
    }

    /// Whether the transaction is just being simulated.
    fn is_simulation(&self) -> bool {
        self.mode() == Mode::SimulateTx
    }

    /// Whether smart contracts should be executed in this context.
    fn should_execute_contracts(&self) -> bool {
        match self.mode() {
            // When actually executing a transaction, we always run contracts.
            Mode::ExecuteTx => true,
            Mode::SimulateTx => {
                // Backwards compatibility for the deprecated `allow_expensive_queries`.
                if let Some(allow_expensive_queries) =
                    self.local_config::<bool>(LOCAL_CONFIG_ALLOW_EXPENSIVE_QUERIES)
                {
                    slog::warn!(
                        self.get_logger("runtime-sdk"),
                        "The {} config option is DEPRECATED since April 2022 and will be removed in a future release. Use {} and {} instead.",
                        LOCAL_CONFIG_ALLOW_EXPENSIVE_QUERIES,
                        LOCAL_CONFIG_ESTIMATE_GAS_BY_SIMULATING_CONTRACTS,
                        LOCAL_CONFIG_ALLOWED_QUERIES
                    );
                    return allow_expensive_queries;
                };

                // The non-deprecated config option.
                self.local_config(LOCAL_CONFIG_ESTIMATE_GAS_BY_SIMULATING_CONTRACTS)
                    .unwrap_or_default()
            }
            // When just checking a transaction, we always want to be fast and skip contracts.
            Mode::CheckTx | Mode::PreScheduleTx => false,
        }
    }

    /// Whether `method` is an allowed query per policy in the local config.
    fn is_allowed_query<R: crate::runtime::Runtime>(&self, method: &str) -> bool {
        let config: Vec<BTreeMap<String, bool>> = self
            .local_config(LOCAL_CONFIG_ALLOWED_QUERIES)
            .unwrap_or_default();
        let is_expensive = R::Modules::is_expensive_query(method);

        // Backwards compatibility for the deprecated `allow_expensive_queries`.
        if let Some(allow_expensive_queries) =
            self.local_config::<bool>(LOCAL_CONFIG_ALLOW_EXPENSIVE_QUERIES)
        {
            slog::warn!(
                self.get_logger("runtime-sdk"),
                "The {} config option is DEPRECATED since April 2022 and will be removed in a future release. Use {} and {} instead.",
                LOCAL_CONFIG_ALLOW_EXPENSIVE_QUERIES,
                LOCAL_CONFIG_ESTIMATE_GAS_BY_SIMULATING_CONTRACTS,
                LOCAL_CONFIG_ALLOWED_QUERIES
            );
            return (!is_expensive) || allow_expensive_queries;
        };

        // The non-deprecated config option.
        config
            .iter()
            .find_map(|item| {
                item.get(method)
                    .or_else(|| {
                        if !is_expensive {
                            return None;
                        }
                        item.get(LOCAL_CONFIG_ALLOWED_QUERIES_ALL_EXPENSIVE)
                    })
                    .or_else(|| item.get(LOCAL_CONFIG_ALLOWED_QUERIES_ALL))
                    .copied()
            })
            // If no config entry matches, the default is to allow only non-expensive queries.
            .unwrap_or(!is_expensive)
    }

    /// Returns node operator-provided local configuration.
    ///
    /// This method will always return `None` in `Mode::ExecuteTx` contexts.
    fn local_config<T>(&self, key: &str) -> Option<T>
    where
        T: cbor::Decode,
    {
        if self.mode() == Mode::ExecuteTx {
            return None;
        }

        self.host_info().local_config.get(key).and_then(|v| {
            cbor::from_value(v.clone()).unwrap_or_else(|e| {
                let msg = format!(
                    "Cannot interpret the value of \"{}\" in runtime's local config as a {}: {:?}",
                    key,
                    std::any::type_name::<T>(),
                    e
                );
                slog::error!(self.get_logger("runtime-sdk"), "{}", msg);
                panic!("{}", msg);
            })
        })
    }

    /// Information about the host environment.
    fn host_info(&self) -> &HostInfo;

    /// Runtime ID.
    fn runtime_id(&self) -> &Namespace {
        &self.host_info().runtime_id
    }

    /// The key manager, if the runtime is confidential.
    fn key_manager(&self) -> Option<&dyn KeyManager>;

    /// Last runtime block header.
    fn runtime_header(&self) -> &roothash::Header;

    /// Results of executing the last successful runtime round.
    fn runtime_round_results(&self) -> &roothash::RoundResults;

    /// Runtime state store.
    fn runtime_state(&mut self) -> &mut Self::Store;

    /// Consensus state.
    fn consensus_state(&self) -> &consensus::state::ConsensusState;

    /// Current epoch.
    fn epoch(&self) -> consensus::beacon::EpochTime;

    /// Emits an event by transforming it into a tag and emitting a tag.
    fn emit_event<E: Event>(&mut self, event: E);

    /// Emits a tag.
    fn emit_etag(&mut self, etag: EventTag);

    /// Emits event tags.
    fn emit_etags(&mut self, etags: EventTags);

    /// Returns a child io_ctx.
    fn io_ctx(&self) -> IoContext;

    /// Commit any changes made to storage, return any emitted tags and runtime messages. It
    /// consumes the transaction context.
    fn commit(
        self,
    ) -> (
        EventTags,
        Vec<(roothash::Message, MessageEventHookInvocation)>,
    );

    /// Rollback any changes made by this context. This method only needs to be called explicitly
    /// in case you want to retrieve possibly emitted unconditional events. Simply dropping the
    /// context without calling `commit` will also result in a rollback.
    fn rollback(self) -> EventTags;

    /// Fetches a value entry associated with the context.
    fn value<V: Any>(&mut self, key: &'static str) -> ContextValue<'_, V>;

    /// Number of consensus messages that can still be emitted.
    fn remaining_messages(&self) -> u32;

    /// Set an upper limit on the number of consensus messages that can be emitted in this context.
    /// Note that the limit can only be decreased and calling this function will return an error
    /// in case the passed `max_messages` is higher than the current limit.
    fn limit_max_messages(&mut self, max_messages: u32) -> Result<(), Error>;

    /// Executes a function in a child context with the given mode.
    ///
    /// The context collects its own messages and starts with an empty set of context values.
    fn with_child<F, Rs>(&mut self, mode: Mode, f: F) -> Rs
    where
        F: FnOnce(
            RuntimeBatchContext<'_, Self::Runtime, storage::OverlayStore<&mut dyn Store>>,
        ) -> Rs;

    /// Executes a function in a simulation context.
    ///
    /// The simulation context collects its own messages and starts with an empty set of context
    /// values.
    fn with_simulation<F, Rs>(&mut self, f: F) -> Rs
    where
        F: FnOnce(
            RuntimeBatchContext<'_, Self::Runtime, storage::OverlayStore<&mut dyn Store>>,
        ) -> Rs,
    {
        self.with_child(Mode::SimulateTx, f)
    }

    /// Returns a random number generator, if it is available, with optional personalization.
    fn rng(&mut self, pers: &[u8]) -> Result<Rng, Error>;

    fn set_tx(&mut self, tx: &[u8]) -> ();
    fn get_tx(&self) -> &[u8];
}

impl<'a, 'b, C: Context> Context for std::cell::RefMut<'a, &'b mut C> {
    type Runtime = C::Runtime;
    type Store = C::Store;

    fn get_logger(&self, module: &'static str) -> slog::Logger {
        self.deref().get_logger(module)
    }

    fn mode(&self) -> Mode {
        self.deref().mode()
    }

    fn host_info(&self) -> &HostInfo {
        self.deref().host_info()
    }

    fn key_manager(&self) -> Option<&dyn KeyManager> {
        self.deref().key_manager()
    }

    fn runtime_header(&self) -> &roothash::Header {
        self.deref().runtime_header()
    }

    fn runtime_round_results(&self) -> &roothash::RoundResults {
        self.deref().runtime_round_results()
    }

    fn runtime_state(&mut self) -> &mut Self::Store {
        self.deref_mut().runtime_state()
    }

    fn consensus_state(&self) -> &consensus::state::ConsensusState {
        self.deref().consensus_state()
    }

    fn epoch(&self) -> consensus::beacon::EpochTime {
        self.deref().epoch()
    }

    fn emit_event<E: Event>(&mut self, event: E) {
        self.deref_mut().emit_event(event);
    }

    fn emit_etag(&mut self, etag: EventTag) {
        self.deref_mut().emit_etag(etag);
    }

    fn emit_etags(&mut self, etags: EventTags) {
        self.deref_mut().emit_etags(etags);
    }

    fn io_ctx(&self) -> IoContext {
        self.deref().io_ctx()
    }

    fn commit(
        self,
    ) -> (
        EventTags,
        Vec<(roothash::Message, MessageEventHookInvocation)>,
    ) {
        unimplemented!()
    }

    fn rollback(self) -> EventTags {
        unimplemented!()
    }

    fn value<V: Any>(&mut self, key: &'static str) -> ContextValue<'_, V> {
        self.deref_mut().value(key)
    }

    fn remaining_messages(&self) -> u32 {
        self.deref().remaining_messages()
    }

    fn limit_max_messages(&mut self, max_messages: u32) -> Result<(), Error> {
        self.deref_mut().limit_max_messages(max_messages)
    }

    fn with_child<F, Rs>(&mut self, mode: Mode, f: F) -> Rs
    where
        F: FnOnce(
            RuntimeBatchContext<'_, Self::Runtime, storage::OverlayStore<&mut dyn Store>>,
        ) -> Rs,
    {
        self.deref_mut().with_child(mode, f)
    }

    fn rng(&mut self, pers: &[u8]) -> Result<Rng, Error> {
        self.deref_mut().rng(pers)
    }

    fn set_tx(&mut self, tx: &[u8]) -> () {
        self.deref_mut().set_tx(tx);
    }
    fn get_tx(&self) -> &[u8] {
        self.deref().get_tx()
    }

}

/// Runtime SDK batch-wide context.
pub trait BatchContext: Context {
    /// Executes a function in a per-transaction context.
    fn with_tx<F, Rs>(
        &mut self,
        tx_index: usize,
        tx_size: u32,
        tx: transaction::Transaction,
        f: F,
    ) -> Rs
    where
        F: FnOnce(
            RuntimeTxContext<'_, '_, <Self as Context>::Runtime, <Self as Context>::Store>,
            transaction::Call,
        ) -> Rs;

    /// Emit consensus messages.
    fn emit_messages(
        &mut self,
        msgs: Vec<(roothash::Message, MessageEventHookInvocation)>,
    ) -> Result<(), Error>;
}

/// Runtime SDK transaction context.
pub trait TxContext: Context {
    /// The index of the transaction in the batch.
    fn tx_index(&self) -> usize;

    /// Transaction size in bytes.
    fn tx_size(&self) -> u32;

    /// Transaction authentication information.
    fn tx_auth_info(&self) -> &transaction::AuthInfo;

    /// The transaction's call format.
    fn tx_call_format(&self) -> transaction::CallFormat;

    /// Whether the call is read-only and must not make any storage modifications.
    fn is_read_only(&self) -> bool;

    /// Whether the transaction is internally generated (e.g. by another module in the SDK as a
    /// subcall to a different module).
    fn is_internal(&self) -> bool;

    /// Mark this context as part of an internally generated transaction (e.g. a subcall).
    fn internal(self) -> Self;

    /// Authenticated address of the caller.
    ///
    /// In case there are multiple signers of a transaction, this will return the address
    /// corresponding to the first signer.
    fn tx_caller_address(&self) -> Address {
        self.tx_auth_info().signer_info[0].address_spec.address()
    }

    /// Fetches an entry pointing to a value associated with the transaction.
    fn tx_value<V: Any>(&mut self, key: &'static str) -> ContextValue<'_, V>;

    /// Emit a consensus message.
    fn emit_message(
        &mut self,
        msg: roothash::Message,
        hook: MessageEventHookInvocation,
    ) -> Result<(), Error>;

    /// Similar as `emit_event` but the event will persist even in case the transaction that owns
    /// this context fails.
    fn emit_unconditional_event<E: Event>(&mut self, event: E);
}

/// Dispatch context for the whole batch.
pub struct RuntimeBatchContext<'a, R: runtime::Runtime, S: NestedStore> {
    mode: Mode,

    host_info: &'a HostInfo,
    key_manager: Option<Box<dyn KeyManager>>,
    runtime_header: &'a roothash::Header,
    runtime_round_results: &'a roothash::RoundResults,
    runtime_storage: S,
    // TODO: linked consensus layer block
    consensus_state: &'a consensus::state::ConsensusState,
    epoch: consensus::beacon::EpochTime,
    io_ctx: Arc<IoContext>,
    logger: slog::Logger,

    /// Whether this context is part of an existing transaction (e.g. a subcall).
    internal: bool,

    /// Block emitted event tags. Events are aggregated by tag key, the value
    /// is a list of all emitted event values.
    block_etags: EventTags,

    /// Maximum number of messages that can be emitted.
    max_messages: u32,
    /// Emitted messages.
    messages: Vec<(roothash::Message, MessageEventHookInvocation)>,

    /// Per-context values.
    values: BTreeMap<&'static str, Box<dyn Any>>,

    rng: Option<Rng>,

    _runtime: PhantomData<R>,

    tx: Vec<u8>,
}

impl<'a, R: runtime::Runtime, S: NestedStore> RuntimeBatchContext<'a, R, S> {
    /// Create a new dispatch context.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mode: Mode,
        host_info: &'a HostInfo,
        key_manager: Option<Box<dyn KeyManager>>,
        runtime_header: &'a roothash::Header,
        runtime_round_results: &'a roothash::RoundResults,
        runtime_storage: S,
        consensus_state: &'a consensus::state::ConsensusState,
        epoch: consensus::beacon::EpochTime,
        io_ctx: Arc<IoContext>,
        max_messages: u32,
    ) -> Self {
        Self {
            mode,
            host_info,
            runtime_header,
            runtime_round_results,
            runtime_storage,
            consensus_state,
            epoch,
            io_ctx,
            key_manager,
            logger: get_logger("runtime-sdk")
                .new(o!("ctx" => "dispatch", "mode" => Into::<&'static str>::into(&mode))),
            internal: false,
            block_etags: EventTags::new(),
            max_messages,
            messages: Vec::new(),
            values: BTreeMap::new(),
            rng: Default::default(),
            _runtime: PhantomData,
            tx: vec![],
        }
    }

    /// Create a new dispatch context from the low-level runtime context.
    pub(crate) fn from_runtime(
        ctx: &'a mut RuntimeContext<'_>,
        host_info: &'a HostInfo,
        key_manager: Option<Box<dyn KeyManager>>,
    ) -> RuntimeBatchContext<'a, R, storage::MKVSStore<&'a mut dyn mkvs::MKVS>> {
        let mode = if ctx.check_only {
            Mode::CheckTx
        } else {
            Mode::ExecuteTx
        };
        RuntimeBatchContext {
            mode,
            host_info,
            key_manager,
            runtime_header: ctx.header,
            runtime_round_results: ctx.round_results,
            runtime_storage: storage::MKVSStore::new(ctx.io_ctx.clone(), ctx.runtime_state),
            consensus_state: &ctx.consensus_state,
            epoch: ctx.epoch,
            io_ctx: ctx.io_ctx.clone(),
            logger: get_logger("runtime-sdk")
                .new(o!("ctx" => "dispatch", "mode" => Into::<&'static str>::into(&mode))),
            internal: false,
            block_etags: EventTags::new(),
            max_messages: ctx.max_messages,
            messages: Vec::new(),
            values: BTreeMap::new(),
            rng: Default::default(),
            _runtime: PhantomData,
            tx: vec![],
        }
    }
}

impl<'a, R: runtime::Runtime, S: NestedStore> Context for RuntimeBatchContext<'a, R, S> {
    type Runtime = R;
    type Store = S;

    fn get_logger(&self, module: &'static str) -> slog::Logger {
        self.logger.new(o!("sdk_module" => module))
    }

    fn mode(&self) -> Mode {
        self.mode
    }

    fn host_info(&self) -> &HostInfo {
        self.host_info
    }

    fn key_manager(&self) -> Option<&dyn KeyManager> {
        self.key_manager.as_ref().map(Box::as_ref)
    }

    fn runtime_header(&self) -> &roothash::Header {
        self.runtime_header
    }

    fn runtime_round_results(&self) -> &roothash::RoundResults {
        self.runtime_round_results
    }

    fn runtime_state(&mut self) -> &mut Self::Store {
        &mut self.runtime_storage
    }

    fn consensus_state(&self) -> &consensus::state::ConsensusState {
        self.consensus_state
    }

    fn epoch(&self) -> consensus::beacon::EpochTime {
        self.epoch
    }

    fn emit_event<E: Event>(&mut self, event: E) {
        let etag = event.into_event_tag();
        let tag = self.block_etags.entry(etag.key).or_insert_with(Vec::new);
        tag.push(etag.value);
    }

    fn emit_etag(&mut self, etag: EventTag) {
        let tag = self.block_etags.entry(etag.key).or_insert_with(Vec::new);
        tag.push(etag.value);
    }

    fn emit_etags(&mut self, etags: EventTags) {
        for (key, val) in etags {
            let tag = self.block_etags.entry(key).or_insert_with(Vec::new);
            tag.extend(val)
        }
    }

    fn io_ctx(&self) -> IoContext {
        IoContext::create_child(&self.io_ctx)
    }

    fn commit(
        self,
    ) -> (
        EventTags,
        Vec<(roothash::Message, MessageEventHookInvocation)>,
    ) {
        self.runtime_storage.commit();
        (self.block_etags, self.messages)
    }

    fn rollback(self) -> EventTags {
        EventTags::new()
    }

    fn value<V: Any>(&mut self, key: &'static str) -> ContextValue<'_, V> {
        ContextValue::new(self.values.entry(key))
    }

    fn remaining_messages(&self) -> u32 {
        self.max_messages.saturating_sub(self.messages.len() as u32)
    }

    fn limit_max_messages(&mut self, max_messages: u32) -> Result<(), Error> {
        if max_messages > self.max_messages {
            return Err(Error::OutOfMessageSlots);
        }

        self.max_messages = max_messages;
        Ok(())
    }

    fn with_child<F, Rs>(&mut self, mode: Mode, f: F) -> Rs
    where
        F: FnOnce(
            RuntimeBatchContext<'_, Self::Runtime, storage::OverlayStore<&mut dyn Store>>,
        ) -> Rs,
    {
        let remaining_messages = self.remaining_messages();
        // Create a store wrapped by an overlay store so any state changes don't leak.
        let store = storage::OverlayStore::new((&mut self.runtime_storage) as &mut dyn Store);

        let child_ctx = RuntimeBatchContext {
            mode,
            host_info: self.host_info,
            key_manager: self.key_manager.clone(),
            runtime_header: self.runtime_header,
            runtime_round_results: self.runtime_round_results,
            runtime_storage: store,
            consensus_state: self.consensus_state,
            epoch: self.epoch,
            io_ctx: self.io_ctx.clone(),
            logger: self
                .logger
                .new(o!("ctx" => "dispatch", "mode" => Into::<&'static str>::into(&mode))),
            internal: self.internal,
            block_etags: EventTags::new(),
            max_messages: match mode {
                Mode::SimulateTx => self.max_messages,
                _ => remaining_messages,
            },
            messages: Vec::new(),
            values: BTreeMap::new(),
            rng: self.rng.as_mut().map(|rng| rng.fork(&[])),
            _runtime: PhantomData,
            tx: self.tx.clone(),
        };
        f(child_ctx)
    }

    fn rng(&mut self, pers: &[u8]) -> Result<Rng, Error> {
        if self.rng.is_none() {
            self.rng = Some(Rng::new(self)?);
        }
        Ok(self.rng.as_mut().unwrap().fork(pers))
    }

    fn set_tx(&mut self, tx: &[u8]) -> () {
        if self.tx.len() != tx.len() {
            self.tx.reserve(tx.len());
            self.tx.truncate(0);
            self.tx.extend_from_slice(tx);
        } else {
            self.tx.clone_from_slice(tx);
        }
    }
    fn get_tx(&self) -> &[u8] {
        &self.tx
    }

}

impl<'a, R: runtime::Runtime, S: NestedStore> BatchContext for RuntimeBatchContext<'a, R, S> {
    fn with_tx<F, Rs>(
        &mut self,
        tx_index: usize,
        tx_size: u32,
        tx: transaction::Transaction,
        f: F,
    ) -> Rs
    where
        F: FnOnce(
            RuntimeTxContext<'_, '_, <Self as Context>::Runtime, <Self as Context>::Store>,
            transaction::Call,
        ) -> Rs,
    {
        let remaining_messages = self.remaining_messages();
        // Create a store wrapped by an overlay store so we can either rollback or commit.
        let store = storage::OverlayStore::new(&mut self.runtime_storage);

        let tx_ctx = RuntimeTxContext {
            mode: self.mode,
            host_info: self.host_info,
            key_manager: self.key_manager.clone(),
            runtime_header: self.runtime_header,
            runtime_round_results: self.runtime_round_results,
            consensus_state: self.consensus_state,
            epoch: self.epoch,
            store,
            io_ctx: self.io_ctx.clone(),
            logger: self
                .logger
                .new(o!("ctx" => "transaction", "mode" => Into::<&'static str>::into(&self.mode))),
            tx_index,
            tx_size,
            tx_auth_info: tx.auth_info,
            tx_call_format: tx.call.format,
            read_only: tx.call.read_only,
            internal: self.internal,
            etags: BTreeMap::new(),
            etags_unconditional: BTreeMap::new(),
            max_messages: remaining_messages,
            messages: Vec::new(),
            values: &mut self.values,
            tx_values: BTreeMap::new(),
            rng: self.rng.as_mut().map(|rng| rng.fork(&[])),
            _runtime: PhantomData,
            tx: self.tx.clone(),
        };
        f(tx_ctx, tx.call)
    }

    fn emit_messages(
        &mut self,
        msgs: Vec<(roothash::Message, MessageEventHookInvocation)>,
    ) -> Result<(), Error> {
        if self.messages.len() + msgs.len() > self.max_messages as usize {
            return Err(Error::OutOfMessageSlots);
        }

        self.messages.extend(msgs);

        Ok(())
    }
}

/// Per-transaction/method dispatch sub-context.
pub struct RuntimeTxContext<'round, 'store, R: runtime::Runtime, S: Store> {
    mode: Mode,

    host_info: &'round HostInfo,
    key_manager: Option<Box<dyn KeyManager>>,
    runtime_header: &'round roothash::Header,
    runtime_round_results: &'round roothash::RoundResults,
    consensus_state: &'round consensus::state::ConsensusState,
    epoch: consensus::beacon::EpochTime,
    // TODO: linked consensus layer block
    store: storage::OverlayStore<&'store mut S>,
    io_ctx: Arc<IoContext>,
    logger: slog::Logger,

    /// The index of the transaction in the block.
    tx_index: usize,
    /// Transaction size.
    tx_size: u32,
    /// Transaction authentication info.
    tx_auth_info: transaction::AuthInfo,
    /// The transaction call format (as received, before decoding by the dispatcher).
    tx_call_format: transaction::CallFormat,
    /// Whether the call is read-only and must not make any storage modifications.
    read_only: bool,
    /// Whether this context is part of an existing transaction (e.g. a subcall).
    internal: bool,

    /// Emitted event tags. Events are aggregated by tag key, the value
    /// is a list of all emitted event values.
    etags: EventTags,
    /// Emitted unconditional event tags.
    etags_unconditional: EventTags,

    /// Maximum number of messages that can be emitted.
    max_messages: u32,
    /// Emitted messages and respective event hooks.
    messages: Vec<(roothash::Message, MessageEventHookInvocation)>,

    /// Per-context values.
    values: &'store mut BTreeMap<&'static str, Box<dyn Any>>,

    /// Per-transaction values.
    tx_values: BTreeMap<&'static str, Box<dyn Any>>,

    /// The RNG associated with the context.
    rng: Option<Rng>,

    _runtime: PhantomData<R>,

    tx: Vec<u8>,
}

impl<'round, 'store, R: runtime::Runtime, S: Store> Context
    for RuntimeTxContext<'round, 'store, R, S>
{
    type Runtime = R;
    type Store = storage::OverlayStore<&'store mut S>;

    fn get_logger(&self, module: &'static str) -> slog::Logger {
        self.logger.new(o!("sdk_module" => module))
    }

    fn mode(&self) -> Mode {
        self.mode
    }

    fn host_info(&self) -> &HostInfo {
        self.host_info
    }

    fn key_manager(&self) -> Option<&dyn KeyManager> {
        self.key_manager.as_ref().map(Box::as_ref)
    }

    fn runtime_header(&self) -> &roothash::Header {
        self.runtime_header
    }

    fn runtime_round_results(&self) -> &roothash::RoundResults {
        self.runtime_round_results
    }

    fn runtime_state(&mut self) -> &mut Self::Store {
        &mut self.store
    }

    fn consensus_state(&self) -> &consensus::state::ConsensusState {
        self.consensus_state
    }

    fn epoch(&self) -> consensus::beacon::EpochTime {
        self.epoch
    }

    fn emit_event<E: Event>(&mut self, event: E) {
        let etag = event.into_event_tag();
        let tag = self.etags.entry(etag.key).or_insert_with(Vec::new);
        tag.push(etag.value);
    }

    fn emit_etag(&mut self, etag: EventTag) {
        let tag = self.etags.entry(etag.key).or_insert_with(Vec::new);
        tag.push(etag.value);
    }

    fn emit_etags(&mut self, etags: EventTags) {
        for (key, val) in etags {
            let tag = self.etags.entry(key).or_insert_with(Vec::new);
            tag.extend(val)
        }
    }

    fn io_ctx(&self) -> IoContext {
        IoContext::create_child(&self.io_ctx)
    }

    fn commit(
        mut self,
    ) -> (
        EventTags,
        Vec<(roothash::Message, MessageEventHookInvocation)>,
    ) {
        // Merge unconditional events into regular events on success.
        for (key, val) in self.etags_unconditional {
            let tag = self.etags.entry(key).or_insert_with(Vec::new);
            tag.extend(val)
        }

        self.store.commit();
        (self.etags, self.messages)
    }

    fn rollback(self) -> EventTags {
        self.etags_unconditional
    }

    fn value<V: Any>(&mut self, key: &'static str) -> ContextValue<'_, V> {
        ContextValue::new(self.values.entry(key))
    }

    fn remaining_messages(&self) -> u32 {
        self.max_messages.saturating_sub(self.messages.len() as u32)
    }

    fn limit_max_messages(&mut self, max_messages: u32) -> Result<(), Error> {
        if max_messages > self.max_messages {
            return Err(Error::OutOfMessageSlots);
        }

        self.max_messages = max_messages;
        Ok(())
    }

    fn with_child<F, Rs>(&mut self, mode: Mode, f: F) -> Rs
    where
        F: FnOnce(
            RuntimeBatchContext<'_, Self::Runtime, storage::OverlayStore<&mut dyn Store>>,
        ) -> Rs,
    {
        let remaining_messages = self.remaining_messages();
        // Create a store wrapped by an overlay store so any state changes don't leak.
        let store = storage::OverlayStore::new((&mut self.store) as &mut dyn Store);

        let child_ctx = RuntimeBatchContext {
            mode,
            host_info: self.host_info,
            key_manager: self.key_manager.clone(),
            runtime_header: self.runtime_header,
            runtime_round_results: self.runtime_round_results,
            runtime_storage: store,
            consensus_state: self.consensus_state,
            epoch: self.epoch,
            io_ctx: self.io_ctx.clone(),
            logger: self
                .logger
                .new(o!("ctx" => "dispatch", "mode" => Into::<&'static str>::into(&mode))),
            internal: self.internal,
            block_etags: EventTags::new(),
            max_messages: match mode {
                Mode::SimulateTx => self.max_messages,
                _ => remaining_messages,
            },
            messages: Vec::new(),
            values: BTreeMap::new(),
            rng: self.rng.as_mut().map(|rng| rng.fork(&[])),
            _runtime: PhantomData,
            tx: self.tx.clone(),
        };
        f(child_ctx)
    }

    fn rng(&mut self, pers: &[u8]) -> Result<Rng, Error> {
        if self.rng.is_none() {
            self.rng = Some(Rng::new(self)?);
        }
        Ok(self.rng.as_mut().unwrap().fork(pers))
    }

    fn set_tx(&mut self, _tx: &[u8]) -> () {
    }
    fn get_tx(&self) -> &[u8] {
        &self.tx
    }
}

impl<R: runtime::Runtime, S: Store> TxContext for RuntimeTxContext<'_, '_, R, S> {
    fn tx_index(&self) -> usize {
        self.tx_index
    }

    fn tx_size(&self) -> u32 {
        self.tx_size
    }

    fn tx_call_format(&self) -> transaction::CallFormat {
        self.tx_call_format
    }

    fn tx_auth_info(&self) -> &transaction::AuthInfo {
        &self.tx_auth_info
    }

    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn is_internal(&self) -> bool {
        self.internal
    }

    fn internal(mut self) -> Self {
        self.internal = true;
        self
    }

    fn tx_value<V: Any>(&mut self, key: &'static str) -> ContextValue<'_, V> {
        ContextValue::new(self.tx_values.entry(key))
    }

    fn emit_message(
        &mut self,
        msg: roothash::Message,
        hook: MessageEventHookInvocation,
    ) -> Result<(), Error> {
        // Check against maximum number of messages that can be emitted per round.
        if self.messages.len() >= self.max_messages as usize {
            return Err(Error::OutOfMessageSlots);
        }

        self.messages.push((msg, hook));

        Ok(())
    }

    fn emit_unconditional_event<E: Event>(&mut self, event: E) {
        let etag = event.into_event_tag();
        let tag = self
            .etags_unconditional
            .entry(etag.key)
            .or_insert_with(Vec::new);
        tag.push(etag.value);
    }
}

/// A per-context arbitrary value.
pub struct ContextValue<'a, V> {
    inner: Entry<'a, &'static str, Box<dyn Any>>,
    _value: PhantomData<V>,
}

impl<'a, V: Any> ContextValue<'a, V> {
    fn new(inner: Entry<'a, &'static str, Box<dyn Any>>) -> Self {
        Self {
            inner,
            _value: PhantomData,
        }
    }

    /// Gets a reference to the specified per-context value.
    ///
    /// # Panics
    ///
    /// Panics if the retrieved type is not the type that was stored.
    pub fn get(self) -> Option<&'a V> {
        match self.inner {
            Entry::Occupied(oe) => Some(
                oe.into_mut()
                    .downcast_ref()
                    .expect("type should stay the same"),
            ),
            _ => None,
        }
    }

    /// Gets a mutable reference to the specified per-context value.
    ///
    /// # Panics
    ///
    /// Panics if the retrieved type is not the type that was stored.
    pub fn get_mut(&mut self) -> Option<&mut V> {
        match &mut self.inner {
            Entry::Occupied(oe) => Some(
                oe.get_mut()
                    .downcast_mut()
                    .expect("type should stay the same"),
            ),
            _ => None,
        }
    }

    /// Sets the context value, returning a mutable reference to the set value.
    ///
    /// # Panics
    ///
    /// Panics if the retrieved type is not the type that was stored.
    pub fn set(self, value: V) -> &'a mut V {
        let value = Box::new(value);
        match self.inner {
            Entry::Occupied(mut oe) => {
                oe.insert(value);
                oe.into_mut()
            }
            Entry::Vacant(ve) => ve.insert(value),
        }
        .downcast_mut()
        .expect("type should stay the same")
    }

    /// Takes the context value, if it exists.
    ///
    /// # Panics
    ///
    /// Panics if the retrieved type is not the type that was stored.
    pub fn take(self) -> Option<V> {
        match self.inner {
            Entry::Occupied(oe) => {
                Some(*oe.remove().downcast().expect("type should stay the same"))
            }
            Entry::Vacant(_) => None,
        }
    }
}

impl<'a, V: Any + Default> ContextValue<'a, V> {
    /// Retrieves the existing value or inserts and returns the default.
    ///
    /// # Panics
    ///
    /// Panics if the retrieved type is not the type that was stored.
    pub fn or_default(self) -> &'a mut V {
        match self.inner {
            Entry::Occupied(oe) => oe.into_mut(),
            Entry::Vacant(ve) => ve.insert(Box::<V>::default()),
        }
        .downcast_mut()
        .expect("type should stay the same")
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod test {
    use oasis_core_runtime::{common::versioned::Versioned, consensus::staking};

    use super::*;
    use crate::testing::{mock, mock::Mock};

    #[test]
    fn test_value() {
        let mut mock = Mock::default();
        let mut ctx = mock.create_ctx();

        let x = ctx.value::<u64>("module.TestKey").get();
        assert_eq!(x, None);

        ctx.value::<u64>("module.TestKey").set(42);

        let y = ctx.value::<u64>("module.TestKey").get();
        assert_eq!(y, Some(&42u64));

        let z = ctx.value::<u64>("module.TestKey").take();
        assert_eq!(z, Some(42u64));

        let y = ctx.value::<u64>("module.TestKey").get();
        assert_eq!(y, None);
    }

    #[test]
    #[should_panic]
    fn test_value_type_change() {
        let mut mock = Mock::default();
        let mut ctx = mock.create_ctx();

        ctx.value::<u64>("module.TestKey").or_default();
        ctx.value::<u32>("module.TestKey").get();
    }

    #[test]
    fn test_value_tx_context() {
        let mut mock = Mock::default();
        let mut ctx = mock.create_ctx();

        ctx.value("module.TestKey").set(42u64);

        let tx = transaction::Transaction {
            version: 1,
            call: transaction::Call {
                format: transaction::CallFormat::Plain,
                method: "test".to_owned(),
                ..Default::default()
            },
            auth_info: transaction::AuthInfo {
                signer_info: vec![],
                fee: transaction::Fee {
                    amount: Default::default(),
                    gas: 1000,
                    consensus_messages: 0,
                },
                ..Default::default()
            },
        };
        ctx.with_tx(0, 0, tx.clone(), |mut tx_ctx, _call| {
            let mut y = tx_ctx.value::<u64>("module.TestKey");
            let y = y.get_mut().unwrap();
            assert_eq!(*y, 42);
            *y = 48;

            let a = tx_ctx.tx_value::<u64>("module.TestTxKey").get();
            assert_eq!(a, None);
            tx_ctx.tx_value::<u64>("module.TestTxKey").set(65);

            let b = tx_ctx.tx_value::<u64>("module.TestTxKey").get();
            assert_eq!(b, Some(&65));

            let c = tx_ctx.tx_value::<u64>("module.TestTakeTxKey").or_default();
            *c = 67;
            let d = tx_ctx.tx_value::<u64>("module.TestTakeTxKey").take();
            assert_eq!(d, Some(67));
            let e = tx_ctx.tx_value::<u64>("module.TestTakeTxKey").get();
            assert_eq!(e, None);
        });

        let x = ctx.value::<u64>("module.TestKey").get();
        assert_eq!(x, Some(&48));

        ctx.with_tx(0, 0, tx, |mut tx_ctx, _call| {
            let z = tx_ctx.value::<u64>("module.TestKey").take();
            assert_eq!(z, Some(48));

            let a = tx_ctx.tx_value::<u64>("module.TestTxKey").get();
            assert_eq!(a, None);
        });

        let y = ctx.value::<u64>("module.TestKey").get();
        assert_eq!(y, None);
    }

    #[test]
    #[should_panic]
    fn test_value_tx_context_type_change() {
        let mut mock = Mock::default();
        let mut ctx = mock.create_ctx();

        let x = ctx.value::<u64>("module.TestKey").set(0);
        *x = 42;

        let tx = transaction::Transaction {
            version: 1,
            call: transaction::Call {
                format: transaction::CallFormat::Plain,
                method: "test".to_owned(),
                ..Default::default()
            },
            auth_info: transaction::AuthInfo {
                signer_info: vec![],
                fee: transaction::Fee {
                    amount: Default::default(),
                    gas: 1000,
                    consensus_messages: 0,
                },
                ..Default::default()
            },
        };
        ctx.with_tx(0, 0, tx, |mut tx_ctx, _call| {
            // Changing the type of a key should result in a panic.
            tx_ctx.value::<Option<u32>>("module.TestKey").get();
        });
    }

    #[test]
    fn test_ctx_message_slots() {
        let mut mock = Mock::default();
        let max_messages = mock.max_messages;
        let mut ctx = mock.create_ctx();

        let mut messages = Vec::with_capacity(max_messages as usize);
        for _ in 0..max_messages {
            messages.push((
                roothash::Message::Staking(Versioned::new(
                    0,
                    roothash::StakingMessage::Transfer(staking::Transfer::default()),
                )),
                MessageEventHookInvocation::new("test".to_string(), ""),
            ))
        }

        // Emitting messages should work.
        ctx.emit_messages(messages.clone())
            .expect("message emitting should work");

        assert_eq!(ctx.remaining_messages(), 0);

        // Emitting more messages should fail.
        ctx.emit_messages(messages)
            .expect_err("message emitting should fail");

        assert_eq!(ctx.remaining_messages(), 0);
    }

    #[test]
    fn test_tx_ctx_message_slots() {
        let mut mock = Mock::default();
        let max_messages = mock.max_messages;
        let mut ctx = mock.create_ctx();

        ctx.with_tx(0, 0, mock::transaction(), |mut tx_ctx, _call| {
            for i in 0..max_messages {
                assert_eq!(tx_ctx.remaining_messages(), max_messages - i);

                tx_ctx
                    .emit_message(
                        roothash::Message::Staking(Versioned::new(
                            0,
                            roothash::StakingMessage::Transfer(staking::Transfer::default()),
                        )),
                        MessageEventHookInvocation::new("test".to_string(), ""),
                    )
                    .expect("message should be emitted");

                assert_eq!(tx_ctx.remaining_messages(), max_messages - i - 1);
            }

            // Another message should error.
            tx_ctx
                .emit_message(
                    roothash::Message::Staking(Versioned::new(
                        0,
                        roothash::StakingMessage::Transfer(staking::Transfer::default()),
                    )),
                    MessageEventHookInvocation::new("test".to_string(), ""),
                )
                .expect_err("message emitting should fail");

            assert_eq!(tx_ctx.remaining_messages(), 0);
        });
    }

    #[test]
    fn test_ctx_message_slot_limits() {
        let mut mock = Mock::default();
        let max_messages = mock.max_messages;
        let mut ctx = mock.create_ctx();

        // Increasing the limit should fail.
        assert_eq!(ctx.remaining_messages(), max_messages);
        ctx.limit_max_messages(max_messages * 2)
            .expect_err("increasing the max message limit should fail");
        assert_eq!(ctx.remaining_messages(), max_messages);

        // Limiting to a single message should work.
        ctx.limit_max_messages(1)
            .expect("limiting max_messages should work");
        assert_eq!(ctx.remaining_messages(), 1);

        let messages = vec![(
            roothash::Message::Staking(Versioned::new(
                0,
                roothash::StakingMessage::Transfer(staking::Transfer::default()),
            )),
            MessageEventHookInvocation::new("test".to_string(), ""),
        )];

        // Emitting messages should work.
        ctx.emit_messages(messages.clone())
            .expect("emitting a message should work");
        assert_eq!(ctx.remaining_messages(), 0);

        // Emitting more messages should fail (we set the limit to a single message).
        ctx.emit_messages(messages.clone())
            .expect_err("emitting a message should fail");
        assert_eq!(ctx.remaining_messages(), 0);

        // Also in transaction contexts.
        ctx.with_tx(0, 0, mock::transaction(), |mut tx_ctx, _call| {
            tx_ctx
                .emit_message(messages[0].0.clone(), messages[0].1.clone())
                .expect_err("emitting a message should fail");
            assert_eq!(tx_ctx.remaining_messages(), 0);
        });

        // Also in child contexts.
        ctx.with_child(Mode::ExecuteTx, |mut child_ctx| {
            child_ctx
                .emit_messages(messages.clone())
                .expect_err("emitting a message should fail");
            assert_eq!(child_ctx.remaining_messages(), 0);
        });
    }

    #[test]
    fn test_tx_ctx_message_slot_limits() {
        let mut mock = Mock::default();
        let mut ctx = mock.create_ctx();

        let messages = vec![(
            roothash::Message::Staking(Versioned::new(
                0,
                roothash::StakingMessage::Transfer(staking::Transfer::default()),
            )),
            MessageEventHookInvocation::new("test".to_string(), ""),
        )];

        ctx.with_tx(0, 0, mock::transaction(), |mut tx_ctx, _call| {
            tx_ctx.limit_max_messages(1).unwrap();

            tx_ctx.with_child(tx_ctx.mode(), |mut child_ctx| {
                child_ctx
                    .emit_messages(messages.clone())
                    .expect("emitting a message should work");

                child_ctx
                    .emit_messages(messages.clone())
                    .expect_err("emitting another message should fail");
            });
        });
    }

    #[test]
    fn test_tx_ctx_metadata() {
        let mut mock = Mock::default();
        let mut ctx = mock.create_ctx();
        ctx.with_tx(42, 888, mock::transaction(), |tx_ctx, _call| {
            assert_eq!(tx_ctx.tx_index(), 42);
            assert_eq!(tx_ctx.tx_size(), 888);
        });
    }
}
