//! EVM module.

#![feature(array_chunks)]
#![feature(test)]

pub mod backend;
pub mod derive_caller;
pub mod precompile;
pub mod raw_tx;
mod signed_call;
pub mod state;
pub mod types;

use std::str::FromStr;

use evm::{
    executor::stack::{MemoryStackState, StackExecutor, StackSubstateMetadata},
    Config as EVMConfig,
};
use once_cell::sync::OnceCell;
use thiserror::Error;

use oasis_runtime_sdk::{
    callformat,
    context::{BatchContext, Context, TxContext, Mode},
    error::Error as _,
    dispatcher::INFO_CACHE,
    handler,
    module::{self, Module as _},
    modules::{
        self,
        accounts::API as _,
        core::{Error as CoreError, API as _},
        consensus_accounts::types::{
            ConsensusWithdrawContext,
            ConsensusTransferContext,
        },
        consensus_accounts::{
            Event as _Event,
            CONSENSUS_WITHDRAW_HANDLER,
            CONSENSUS_TRANSFER_HANDLER,
            CallParam,
        },
    },
    runtime::Runtime,
    sdk_derive,
    storage,
    types::{
        address::{self, Address},
        token,
        transaction::{self, Transaction},
        message::MessageEvent,
    },
};

use backend::ApplyBackendResult;
use types::{H160, H256, U256};

fn slice_to_array_32<T>(slice: &[T]) -> &[T; 32] {
    let ptr = slice.as_ptr() as *const [T; 32];
    unsafe {&*ptr}
}

fn u256_to_u128(value: U256) -> Result<u128, Error> {
    if value < U256::from(u128::MAX) {
        Ok(value.low_u128())
    } else {
        Err(Error::Reverted("too large value".to_string()))
    }
}

fn u128_to_h256(v: u128) -> H256 {
    let mut v_vec = v.to_be_bytes().to_vec();

    let z = u128::from(0 as u8);
    let mut z_vec = z.to_be_bytes().to_vec();

    z_vec.append(&mut v_vec);

    let ary32 = slice_to_array_32(z_vec.as_slice());

    H256::from(ary32)
}

const DW_SYSTEM_ADDRESS: &str = "0x052cc647E136C85ED9F6Bf5DBB5E79952Be0499F";
const DW_CONTRACT_ADDRESS: &str = "0xBE75FDe9DeDe700635E3dDBe7e29b5db1A76C125";


#[cfg(test)]
mod test;

/// Unique module name.
const MODULE_NAME: &str = "evm";

/// Module configuration.
pub trait Config: 'static {
    /// AdditionalPrecompileSet is the type used for the additional precompiles.
    /// Use `()` if unused.
    type AdditionalPrecompileSet: evm::executor::stack::PrecompileSet;
    /// Module that is used for accessing accounts.
    type Accounts: modules::accounts::API;

    /// The chain ID to supply when a contract requests it. Ethereum-format transactions must use
    /// this chain ID.
    const CHAIN_ID: u64;

    /// Token denomination used as the native EVM token.
    const TOKEN_DENOMINATION: token::Denomination;

    /// Whether to use confidential storage by default, and transaction data encryption.
    const CONFIDENTIAL: bool = false;

    /// Maps an Ethereum address into an SDK account address.
    fn map_address(address: primitive_types::H160) -> Address {
        Address::new(
            address::ADDRESS_V0_SECP256K1ETH_CONTEXT,
            address::ADDRESS_V0_VERSION,
            address.as_ref(),
        )
    }

    /// Provides additional precompiles that should be available to the EVM.
    ///
    /// If any of the precompile addresses returned is the same as for one of
    /// the builtin precompiles, then the returned implementation will
    /// overwrite the builtin implementation.
    fn additional_precompiles() -> Option<Self::AdditionalPrecompileSet> {
        None
    }

    /// Returns the config used by the EVM (in the hardfork sense).
    // In some cases, the config may be runtime config dependent (e.g., constant
    // timeness when confidential), so this is made part of the trait.
    fn evm_config(estimation: bool) -> &'static EVMConfig {
        static EVM_CONFIG: OnceCell<EVMConfig> = OnceCell::new();
        static EVM_CONFIG_ESTIMATE: OnceCell<EVMConfig> = OnceCell::new();

        if estimation {
            EVM_CONFIG_ESTIMATE.get_or_init(|| {
                // The estimate mode overestimates transaction costs and returns a gas costs
                // that should be sufficient to execute a transaction, but likely overestimated.
                // The "proper" EVM-way to estimate exact gas is to disable this estimation and
                // do a binary search over all possible gas costs to find the minimum gas cost
                // with which the transaction succeeds. This mode should only be used when the
                // caller wants to avoid the expensive binary search and is ok with a possible
                // overestimation of gas costs.
                EVMConfig {
                    estimate: true,
                    ..Self::evm_config(false).clone()
                }
            })
        } else {
            EVM_CONFIG.get_or_init(EVMConfig::london)
        }
    }
}

pub struct Module<Cfg: Config> {
    _cfg: std::marker::PhantomData<Cfg>,
}

/// Errors emitted by the EVM module.
#[derive(Error, Debug, oasis_runtime_sdk::Error)]
pub enum Error {
    #[error("invalid argument")]
    #[sdk_error(code = 1)]
    InvalidArgument,

    #[error("execution failed: {0}")]
    #[sdk_error(code = 2)]
    ExecutionFailed(String),

    #[error("invalid signer type")]
    #[sdk_error(code = 3)]
    InvalidSignerType,

    #[error("fee overflow")]
    #[sdk_error(code = 4)]
    FeeOverflow,

    #[error("gas limit too low: {0} required")]
    #[sdk_error(code = 5)]
    GasLimitTooLow(u64),

    #[error("insufficient balance")]
    #[sdk_error(code = 6)]
    InsufficientBalance,

    #[error("forbidden by policy")]
    #[sdk_error(code = 7)]
    Forbidden,

    #[error("reverted: {0}")]
    #[sdk_error(code = 8)]
    Reverted(String),

    #[error("forbidden by policy: this node only allows simulating calls that use up to {0} gas")]
    #[sdk_error(code = 9)]
    SimulationTooExpensive(u64),

    #[error("invalid signed simulate call query: {0}")]
    #[sdk_error(code = 10)]
    InvalidSignedSimulateCall(&'static str),

    #[error("core: {0}")]
    #[sdk_error(transparent)]
    Core(#[from] CoreError),
}

impl From<evm::ExitError> for Error {
    fn from(e: evm::ExitError) -> Error {
        use evm::ExitError::*;
        let msg = match e {
            StackUnderflow => "stack underflow",
            StackOverflow => "stack overflow",
            InvalidJump => "invalid jump",
            InvalidRange => "invalid range",
            DesignatedInvalid => "designated invalid",
            CallTooDeep => "call too deep",
            CreateCollision => "create collision",
            CreateContractLimit => "create contract limit",
            InvalidCode(..) => "invalid code",

            OutOfOffset => "out of offset",
            OutOfGas => "out of gas",
            OutOfFund => "out of funds",

            #[allow(clippy::upper_case_acronyms)]
            PCUnderflow => "PC underflow",

            CreateEmpty => "create empty",

            Other(msg) => return Error::ExecutionFailed(msg.to_string()),
        };
        Error::ExecutionFailed(msg.to_string())
    }
}

impl From<evm::ExitFatal> for Error {
    fn from(e: evm::ExitFatal) -> Error {
        use evm::ExitFatal::*;
        let msg = match e {
            NotSupported => "not supported",
            UnhandledInterrupt => "unhandled interrupt",
            CallErrorAsFatal(err) => return err.into(),
            Other(msg) => return Error::ExecutionFailed(msg.to_string()),
        };
        Error::ExecutionFailed(msg.to_string())
    }
}

/// Process an EVM result to return either a successful result or a (readable) error reason.
fn process_evm_result(exit_reason: evm::ExitReason, data: Vec<u8>) -> Result<Vec<u8>, Error> {
    match exit_reason {
        evm::ExitReason::Succeed(_) => Ok(data),
        evm::ExitReason::Revert(_) => {
            if data.is_empty() {
                return Err(Error::Reverted("no revert reason".to_string()));
            }

            // Decode revert reason, format is as follows:
            //
            // 08c379a0                                                         <- Function selector
            // 0000000000000000000000000000000000000000000000000000000000000020 <- Offset of string return value
            // 0000000000000000000000000000000000000000000000000000000000000047 <- Length of string return value (the revert reason)
            // 6d7946756e6374696f6e206f6e6c79206163636570747320617267756d656e74 <- First 32 bytes of the revert reason
            // 7320776869636820617265206772656174686572207468616e206f7220657175 <- Next 32 bytes of the revert reason
            // 616c20746f203500000000000000000000000000000000000000000000000000 <- Last 7 bytes of the revert reason
            //
            const ERROR_STRING_SELECTOR: &[u8] = &[0x08, 0xc3, 0x79, 0xa0]; // Keccak256("Error(string)")
            const FIELD_OFFSET_START: usize = 4;
            const FIELD_LENGTH_START: usize = FIELD_OFFSET_START + 32;
            const FIELD_REASON_START: usize = FIELD_LENGTH_START + 32;
            const MIN_SIZE: usize = FIELD_REASON_START;
            const MAX_REASON_SIZE: usize = 1024;

            let max_raw_len = if data.len() > MAX_REASON_SIZE {
                MAX_REASON_SIZE
            } else {
                data.len()
            };
            if data.len() < MIN_SIZE || !data.starts_with(ERROR_STRING_SELECTOR) {
                return Err(Error::Reverted(format!(
                    "invalid reason prefix: '{}'",
                    base64::encode(&data[..max_raw_len])
                )));
            }
            // Decode and validate length.
            let mut length =
                primitive_types::U256::from(&data[FIELD_LENGTH_START..FIELD_LENGTH_START + 32])
                    .low_u32() as usize;
            if FIELD_REASON_START + length > data.len() {
                return Err(Error::Reverted(format!(
                    "invalid reason length: '{}'",
                    base64::encode(&data[..max_raw_len])
                )));
            }
            // Make sure that this doesn't ever return huge reason values as this is at least
            // somewhat contract-controlled.
            if length > MAX_REASON_SIZE {
                length = MAX_REASON_SIZE;
            }
            let reason =
                String::from_utf8_lossy(&data[FIELD_REASON_START..FIELD_REASON_START + length]);
            Err(Error::Reverted(reason.to_string()))
        }
        evm::ExitReason::Error(err) => Err(err.into()),
        evm::ExitReason::Fatal(err) => Err(err.into()),
    }
}

/// Gas costs.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct GasCosts {}

/// Parameters for the EVM module.
#[derive(Clone, Default, Debug, cbor::Encode, cbor::Decode)]
pub struct Parameters {
    /// Gas costs.
    pub gas_costs: GasCosts,
}

impl module::Parameters for Parameters {
    type Error = ();

    fn validate_basic(&self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Genesis state for the EVM module.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct Genesis {
    pub parameters: Parameters,
}

/// Local configuration that can be provided by the node operator.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct LocalConfig {
    /// Maximum gas limit that can be passed to the `evm.SimulateCall` query. Queries
    /// with a higher gas limit will be rejected. A special value of `0` indicates
    /// no limit. Default: 0.
    #[cbor(optional)]
    pub query_simulate_call_max_gas: u64,
}

/// Events emitted by the EVM module.
#[derive(Debug, cbor::Encode, oasis_runtime_sdk::Event)]
#[cbor(untagged)]
pub enum Event {
    #[sdk_event(code = 1)]
    Log {
        address: H160,
        topics: Vec<H256>,
        data: Vec<u8>,
    },
}

impl<Cfg: Config> module::Module for Module<Cfg> {
    const NAME: &'static str = MODULE_NAME;
    type Error = Error;
    type Event = Event;
    type Parameters = Parameters;
}

/// Interface that can be called from other modules.
pub trait API {
    /// Perform an Ethereum CREATE transaction.
    /// Returns 160-bit address of created contract.
    fn create<C: TxContext>(ctx: &mut C, value: U256, init_code: Vec<u8>)
        -> Result<Vec<u8>, Error>;

    /// Perform an Ethereum CALL transaction.
    fn call<C: TxContext>(
        ctx: &mut C,
        address: H160,
        value: U256,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, Error>;

    // GBNOTE: call accounts.transfer directly if data.is_empty.
    fn transfer<C: TxContext>(
        ctx: &mut C,
        address: H160,
        value: U256,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, Error>;

    fn call_sc_mint<C: Context>(
        ctx: &mut C,
        address: &H160,
        amount: &H256,
        by_system: bool,
    ) -> Result<Vec<u8>, Error>;

    fn call_sc_burn<C: Context>(
        ctx: &mut C,
        address: &H160,
        amount: &H256,
        by_system: bool,
    ) -> Result<Vec<u8>, Error>;

    /// Peek into EVM storage.
    /// Returns 256-bit value stored at given contract address and index (slot)
    /// in the storage.
    fn get_storage<C: Context>(ctx: &mut C, address: H160, index: H256) -> Result<Vec<u8>, Error>;

    /// Peek into EVM code storage.
    /// Returns EVM bytecode of contract at given address.
    fn get_code<C: Context>(ctx: &mut C, address: H160) -> Result<Vec<u8>, Error>;

    /// Get EVM account balance.
    fn get_balance<C: Context>(ctx: &mut C, address: H160) -> Result<u128, Error>;

    /// Simulate an Ethereum CALL.
    ///
    /// If the EVM is confidential, it may accept _signed queries_, which are formatted as
    /// an either a [`sdk::types::transaction::Call`] or [`types::SignedCallDataPack`] encoded
    /// and packed into the `data` field of the [`types::SimulateCallQuery`].
    fn simulate_call<C: Context>(
        ctx: &mut C,
        call: types::SimulateCallQuery,
    ) -> Result<Vec<u8>, Error>;
}

impl<Cfg: Config> API for Module<Cfg> {
    fn create<C: TxContext>(
        ctx: &mut C,
        value: U256,
        init_code: Vec<u8>,
    ) -> Result<Vec<u8>, Error> {
        let caller = Self::derive_caller(ctx)?;

        if !ctx.should_execute_contracts() {
            // Only fast checks are allowed.
            return Ok(vec![]);
        }

        // Create output (the contract address) does not need to be encrypted because it's
        // trivially computable by anyone who can observe the create tx and receipt status.
        // Therefore, we don't need the `tx_metadata` or to encode the result.
        let (init_code, _tx_metadata) =
            Self::decode_call_data(ctx, init_code, ctx.tx_call_format(), ctx.tx_index(), true)?
                .expect("processing always proceeds");

        Self::do_evm(
            caller,
            ctx,
            |exec, gas_limit| {
                let address = exec.create_address(evm::CreateScheme::Legacy {
                    caller: caller.into(),
                });
                let (exit_reason, exit_value) =
                    exec.transact_create(caller.into(), value.into(), init_code, gas_limit, vec![]);
                if exit_reason.is_succeed() {
                    // If successful return the contract deployed address.
                    (exit_reason, address.as_bytes().to_vec())
                } else {
                    // Otherwise propagate the exit value.
                    (exit_reason, exit_value)
                }
            },
            // If in simulation, this must be EstimateGas query.
            // Use estimate mode if not doing binary search for exact gas costs.
            ctx.is_simulation()
                && <C::Runtime as Runtime>::Core::estimate_gas_search_max_iters(ctx) == 0,
        )
    }

    fn transfer<C: TxContext>(
        ctx: &mut C,
        address: H160,
        value: U256,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, Error> {
        if !ctx.should_execute_contracts() {
            // Only fast checks are allowed.
            return Ok(vec![]);
        }

        let caller = Self::derive_caller(ctx)?;

        let from = Cfg::map_address(caller.into());

        let to = Cfg::map_address(address.into());

        let u128value = u256_to_u128(value)?;
        let amount = token::BaseUnits::new(u128value, Cfg::TOKEN_DENOMINATION);

        let (_, tx_metadata) =
            Self::decode_call_data(ctx, data, ctx.tx_call_format(), ctx.tx_index(), true)?
                .expect("processing always proceeds");

        let gas_limit: u64 = <C::Runtime as Runtime>::Core::remaining_tx_gas(ctx);
        let gas_price: primitive_types::U256 = ctx.tx_auth_info().fee.gas_price().into();
        let fee_denomination = ctx.tx_auth_info().fee.amount.denomination().clone();

        // The maximum gas fee has already been withdrawn in authenticate_tx().
        let max_gas_fee = gas_price
            .checked_mul(primitive_types::U256::from(gas_limit))
            .ok_or(Error::FeeOverflow)?;

        let gas_used = 21000;
        let fee = gas_price * gas_used;

        let my_result: Result<(), Error> =
            Cfg::Accounts::transfer(ctx, from, to, &amount).map_err(|_| Error::InvalidArgument);

        if my_result.is_err() {
            <C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;
            return Err(my_result.unwrap_err());
        }

        // Return the difference between the pre-paid max_gas and actually used gas.
        let return_fee = max_gas_fee
            .checked_sub(fee)
            .ok_or(Error::InsufficientBalance)?;

        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;

        // Move the difference from the fee accumulator back to the caller.
        let caller_address = Cfg::map_address(caller.into());
        Cfg::Accounts::move_from_fee_accumulator(
            ctx,
            caller_address,
            &token::BaseUnits::new(return_fee.as_u128(), fee_denomination),
        )
        .map_err(|_| Error::InsufficientBalance)?;

        let adapted_result = match my_result {
            Ok(()) => Ok(Vec::new()), // Convert Ok(()) to Ok(Vec::new())
            Err(e) => Err(e),         // Use Err(e) as is
        };

        Self::encode_evm_result(ctx, adapted_result, tx_metadata)
    }

    fn call<C: TxContext>(
        ctx: &mut C,
        address: H160,
        value: U256,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, Error> {
        let caller = Self::derive_caller(ctx)?;

        if !ctx.should_execute_contracts() {
            // Only fast checks are allowed.
            return Ok(vec![]);
        }

        let (data, tx_metadata) =
            Self::decode_call_data(ctx, data, ctx.tx_call_format(), ctx.tx_index(), true)?
                .expect("processing always proceeds");

        let evm_result = Self::do_evm(
            caller,
            ctx,
            |exec, gas_limit| {
                exec.transact_call(
                    caller.into(),
                    address.into(),
                    value.into(),
                    data,
                    gas_limit,
                    vec![],
                )
            },
            // If in simulation, this must be EstimateGas query.
            // Use estimate mode if not doing binary search for exact gas costs.
            ctx.is_simulation()
                && <C::Runtime as Runtime>::Core::estimate_gas_search_max_iters(ctx) == 0,
        );
        Self::encode_evm_result(ctx, evm_result, tx_metadata)
    }

    fn call_sc_mint<C: Context>(
        ctx: &mut C,
        address: &H160,
        amount: &H256,
        by_system: bool,
    ) -> Result<Vec<u8>, Error> {
        //let caller = Self::derive_caller(ctx)?;

        let caller = H160::from_str(DW_SYSTEM_ADDRESS).unwrap();
        let sc_addr = H160::from_str(DW_CONTRACT_ADDRESS).unwrap();
        let flag = if by_system {
            H256::from_low_u64_be(1)
        } else {
            H256::from_low_u64_be(0)
        };

        let mut data = vec![0xd1, 0xa1, 0xbe, 0xb4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];    //mint() signature
        data.append(&mut address.as_bytes().to_vec());
        data.append(&mut amount.as_bytes().to_vec());
        data.append(&mut flag.as_bytes().to_vec());

        if !ctx.should_execute_contracts() {
            // Only fast checks are allowed.
            return Ok(vec![]);
        }

        let tx_metadata = callformat::Metadata::Empty;

        let evm_result = Self::do_sc_evm(
            caller,
            ctx,
            |exec, gas_limit| {
                exec.transact_call(
                    caller.into(),
                    sc_addr.into(),
                    primitive_types::U256::zero(),
                    data,
                    gas_limit,
                    vec![],
                )
            },
            // If in simulation, this must be EstimateGas query.
            // Use estimate mode if not doing binary search for exact gas costs.
            ctx.is_simulation()
                && <C::Runtime as Runtime>::Core::estimate_gas_search_max_iters(ctx) == 0,
        );
        Self::encode_evm_result(ctx, evm_result, tx_metadata)
    }

    fn call_sc_burn<C: Context>(
        ctx: &mut C,
        address: &H160,
        amount: &H256,
        by_system: bool,
    ) -> Result<Vec<u8>, Error> {
        //let caller = Self::derive_caller(ctx)?;

        let caller = H160::from_str(DW_SYSTEM_ADDRESS).unwrap();
        let sc_addr = H160::from_str(DW_CONTRACT_ADDRESS).unwrap();
        let flag = if by_system {
            H256::from_low_u64_be(1)
        } else {
            H256::from_low_u64_be(0)
        };

        let mut data = vec![0x76, 0xfd, 0x4f, 0xdf, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];    //mint() signature
        data.append(&mut address.as_bytes().to_vec());
        data.append(&mut amount.as_bytes().to_vec());
        data.append(&mut flag.as_bytes().to_vec());

        if !ctx.should_execute_contracts() {
            // Only fast checks are allowed.
            return Ok(vec![]);
        }

        let tx_metadata = callformat::Metadata::Empty;

        let evm_result = Self::do_sc_evm(
            caller,
            ctx,
            |exec, gas_limit| {
                exec.transact_call(
                    caller.into(),
                    sc_addr.into(),
                    primitive_types::U256::zero(),
                    data,
                    gas_limit,
                    vec![],
                )
            },
            // If in simulation, this must be EstimateGas query.
            // Use estimate mode if not doing binary search for exact gas costs.
            ctx.is_simulation()
                && <C::Runtime as Runtime>::Core::estimate_gas_search_max_iters(ctx) == 0,
        );
        Self::encode_evm_result(ctx, evm_result, tx_metadata)
    }

    fn get_storage<C: Context>(ctx: &mut C, address: H160, index: H256) -> Result<Vec<u8>, Error> {
        let s = state::public_storage(ctx, &address);
        let result: H256 = s.get(index).unwrap_or_default();
        Ok(result.as_bytes().to_vec())
    }

    fn get_code<C: Context>(ctx: &mut C, address: H160) -> Result<Vec<u8>, Error> {
        let store = storage::PrefixStore::new(ctx.runtime_state(), &crate::MODULE_NAME);
        let codes = storage::TypedStore::new(storage::PrefixStore::new(store, &state::CODES));

        Ok(codes.get(address).unwrap_or_default())
    }

    fn get_balance<C: Context>(ctx: &mut C, address: H160) -> Result<u128, Error> {
        let state = ctx.runtime_state();
        let address = Cfg::map_address(address.into());
        Ok(Cfg::Accounts::get_balance(state, address, Cfg::TOKEN_DENOMINATION).unwrap_or_default())
    }

    fn simulate_call<C: Context>(
        ctx: &mut C,
        call: types::SimulateCallQuery,
    ) -> Result<Vec<u8>, Error> {
        let (
            types::SimulateCallQuery {
                gas_price,
                gas_limit,
                caller,
                address,
                value,
                data,
            },
            tx_metadata,
        ) = Self::decode_simulate_call_query(ctx, call)?;

        let evm_result = ctx.with_simulation(|mut sctx| {
            let call_tx = transaction::Transaction {
                version: 1,
                call: transaction::Call {
                    format: transaction::CallFormat::Plain,
                    method: "evm.Call".to_owned(),
                    body: cbor::to_value(types::Call {
                        address,
                        value,
                        data: data.clone(),
                    }),
                    ..Default::default()
                },
                auth_info: transaction::AuthInfo {
                    signer_info: vec![],
                    fee: transaction::Fee {
                        amount: token::BaseUnits::new(
                            gas_price
                                .checked_mul(U256::from(gas_limit))
                                .ok_or(Error::FeeOverflow)?
                                .as_u128(),
                            Cfg::TOKEN_DENOMINATION,
                        ),
                        gas: gas_limit,
                        consensus_messages: 0,
                    },
                    ..Default::default()
                },
            };
            sctx.with_tx(0, 0, call_tx, |mut txctx, _call| {
                Self::do_evm(
                    caller,
                    &mut txctx,
                    |exec, gas_limit| {
                        exec.transact_call(
                            caller.into(),
                            address.into(),
                            value.into(),
                            data,
                            gas_limit,
                            vec![],
                        )
                    },
                    // Simulate call is never called from EstimateGas.
                    false,
                )
            })
        });
        Self::encode_evm_result(ctx, evm_result, tx_metadata)
    }
}

impl<Cfg: Config> Module<Cfg> {
    fn do_evm<C, F>(source: H160, ctx: &mut C, f: F, estimate_gas: bool) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(
            &mut StackExecutor<
                'static,
                '_,
                MemoryStackState<'_, 'static, backend::Backend<'_, C, Cfg>>,
                precompile::Precompiles<Cfg, backend::Backend<'_, C, Cfg>>,
            >,
            u64,
        ) -> (evm::ExitReason, Vec<u8>),
        C: TxContext,
    {
        let cfg = Cfg::evm_config(estimate_gas);
        let gas_limit: u64 = <C::Runtime as Runtime>::Core::remaining_tx_gas(ctx);
        let gas_price: primitive_types::U256 = ctx.tx_auth_info().fee.gas_price().into();
        let fee_denomination = ctx.tx_auth_info().fee.amount.denomination().clone();

        let vicinity = backend::Vicinity {
            gas_price: gas_price.into(),
            origin: source,
        };

        // The maximum gas fee has already been withdrawn in authenticate_tx().
        let max_gas_fee = gas_price
            .checked_mul(primitive_types::U256::from(gas_limit))
            .ok_or(Error::FeeOverflow)?;

        let mut backend = backend::Backend::<'_, C, Cfg>::new(ctx, vicinity);
        let metadata = StackSubstateMetadata::new(gas_limit, cfg);
        let stackstate = MemoryStackState::new(metadata, &backend);
        let precompiles = precompile::Precompiles::new(&backend);
        let mut executor = StackExecutor::new_with_precompiles(stackstate, cfg, &precompiles);

        // Run EVM and process the result.
        let (exit_reason, exit_value) = f(&mut executor, gas_limit);
        let gas_used = executor.used_gas();
        let fee = executor.fee(gas_price);

        let exit_value = match process_evm_result(exit_reason, exit_value) {
            Ok(exit_value) => exit_value,
            Err(err) => {
                <C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;
                return Err(err);
            }
        };

        // Return the difference between the pre-paid max_gas and actually used gas.
        let return_fee = max_gas_fee
            .checked_sub(fee)
            .ok_or(Error::InsufficientBalance)?;

        let (vals, logs) = executor.into_state().deconstruct();

        // Apply can fail in case of unsupported actions.
        let exit_reason = backend.apply(vals, logs);
        if let Err(err) = process_evm_result(exit_reason, Vec::new()) {
            <C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;
            return Err(err);
        };

        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;

        // Move the difference from the fee accumulator back to the caller.
        let caller_address = Cfg::map_address(source.into());
        Cfg::Accounts::move_from_fee_accumulator(
            ctx,
            caller_address,
            &token::BaseUnits::new(return_fee.as_u128(), fee_denomination),
        )
        .map_err(|_| Error::InsufficientBalance)?;

        Ok(exit_value)
    }

    fn do_sc_evm<C, F>(
        source: H160,
        ctx: &mut C,
        f: F,
        estimate_gas: bool,
    ) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(
            &mut StackExecutor<
                'static,
                '_,
                MemoryStackState<'_, 'static, backend::Backend<'_, C, Cfg>>,
                precompile::Precompiles<Cfg, backend::Backend<'_, C, Cfg>>,
            >,
            u64,
        ) -> (evm::ExitReason, Vec<u8>),
        C: Context,
    {
        let cfg = Cfg::evm_config(estimate_gas);
        let gas_limit: u64 = 1085479;
        let gas_price: primitive_types::U256 = primitive_types::U256::from_str("0x03e8").unwrap(); //primitive_types::U256::zero();
        //let fee_denomination = token::Denomination::NATIVE;

        let vicinity = backend::Vicinity {
            gas_price: gas_price.into(),
            origin: source,
        };

        let mut backend = backend::Backend::<'_, C, Cfg>::new_internal(ctx, vicinity);
        let metadata = StackSubstateMetadata::new(gas_limit, cfg);
        let stackstate = MemoryStackState::new(metadata, &backend);
        let precompiles = precompile::Precompiles::new(&backend);
        let mut executor = StackExecutor::new_with_precompiles(stackstate, cfg, &precompiles);

        // Run EVM and process the result.
        let (exit_reason, exit_value) = f(&mut executor, gas_limit);
        //let gas_used = executor.used_gas();
        //let fee = executor.fee(gas_price);

        let exit_value = match process_evm_result(exit_reason, exit_value) {
            Ok(exit_value) => exit_value,
            Err(err) => {
                //<C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;
                return Err(err);
            }
        };

        // Return the difference between the pre-paid max_gas and actually used gas.
        //let return_fee = 0;

        let (vals, logs) = executor.into_state().deconstruct();

        // Apply can fail in case of unsupported actions.
        let exit_reason = backend.apply(vals, logs);
        if let Err(err) = process_evm_result(exit_reason, Vec::new()) {
            //<C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;
            return Err(err);
        };

        //<C::Runtime as Runtime>::Core::use_tx_gas(ctx, gas_used)?;

        // Move the difference from the fee accumulator back to the caller.
        /*
        let caller_address = Cfg::map_address(source.into());
        Cfg::Accounts::move_from_fee_accumulator(
            ctx,
            caller_address,
            &token::BaseUnits::new(return_fee.as_u128(), fee_denomination),
        )
        .map_err(|_| Error::InsufficientBalance)?;
        */

        Ok(exit_value)
    }

    fn derive_caller<C>(ctx: &mut C) -> Result<H160, Error>
    where
        C: TxContext,
    {
        derive_caller::from_tx_auth_info(ctx.tx_auth_info())
    }

    /// Returns the decrypted call data or `None` if this transaction is simulated in
    /// a context that may not include a key manager (i.e. SimulateCall but not EstimateGas).
    fn decode_call_data<C: Context>(
        ctx: &C,
        data: Vec<u8>,
        format: transaction::CallFormat, // The tx call format.
        tx_index: usize,
        assume_km_reachable: bool,
    ) -> Result<Option<(Vec<u8>, callformat::Metadata)>, Error> {
        if !Cfg::CONFIDENTIAL || format != transaction::CallFormat::Plain {
            // Either the runtime is non-confidential and all txs are plaintext, or the tx
            // is sent using a confidential call format and the tx has already been decrypted.
            return Ok(Some((data, callformat::Metadata::Empty)));
        }
        match cbor::from_slice(&data) {
            Ok(call) => Self::decode_call(ctx, call, tx_index, assume_km_reachable),
            Err(_) => Ok(Some((data, callformat::Metadata::Empty))), // It's not encrypted.
        }
    }

    /// Returns the decrypted call data or `None` if this transaction is simulated in
    /// a context that may not include a key manager (i.e. SimulateCall but not EstimateGas).
    fn decode_call<C: Context>(
        ctx: &C,
        call: transaction::Call,
        tx_index: usize,
        assume_km_reachable: bool,
    ) -> Result<Option<(Vec<u8>, callformat::Metadata)>, Error> {
        match callformat::decode_call_ex(ctx, call, tx_index, assume_km_reachable)? {
            Some((
                transaction::Call {
                    body: cbor::Value::ByteString(data),
                    ..
                },
                metadata,
            )) => Ok(Some((data, metadata))),
            Some((_, _)) => {
                Err(CoreError::InvalidCallFormat(anyhow::anyhow!("invalid inner data")).into())
            }
            None => Ok(None),
        }
    }

    fn decode_simulate_call_query<C: Context>(
        ctx: &mut C,
        call: types::SimulateCallQuery,
    ) -> Result<(types::SimulateCallQuery, callformat::Metadata), Error> {
        if !Cfg::CONFIDENTIAL {
            return Ok((call, callformat::Metadata::Empty));
        }
        if let Ok(types::SignedCallDataPack {
            data,
            leash,
            signature,
        }) = cbor::from_slice(&call.data)
        {
            let (data, tx_metadata) =
                Self::decode_call(ctx, data, 0, true)?.expect("processing always proceeds");
            return Ok((
                signed_call::verify::<_, Cfg>(
                    ctx,
                    types::SimulateCallQuery { data, ..call },
                    leash,
                    signature,
                )?,
                tx_metadata,
            ));
        }

        // The call is not signed, but it must be encoded as an oasis-sdk call.
        let tx_call_format = transaction::CallFormat::Plain; // Queries cannot be encrypted.
        let (data, tx_metadata) = Self::decode_call_data(ctx, call.data, tx_call_format, 0, true)?
            .expect("processing always proceeds");
        Ok((
            types::SimulateCallQuery {
                caller: Default::default(), // The sender cannot be spoofed.
                data,
                ..call
            },
            tx_metadata,
        ))
    }

    fn encode_evm_result<C: Context>(
        ctx: &C,
        evm_result: Result<Vec<u8>, Error>,
        tx_metadata: callformat::Metadata, // Potentially parsed from an inner enveloped tx.
    ) -> Result<Vec<u8>, Error> {
        if matches!(tx_metadata, callformat::Metadata::Empty) {
            // Either the runtime is non-confidential and all responses are plaintext,
            // or the tx was sent using a confidential call format and dispatcher will
            // encrypt the call in the normal way.
            return evm_result;
        }
        let call_result = match evm_result {
            Ok(exit_value) => module::CallResult::Ok(exit_value.into()),
            Err(e) => module::CallResult::Failed {
                module: e.module_name().into(),
                code: e.code(),
                message: e.to_string(),
            },
        };
        Ok(cbor::to_vec(callformat::encode_result(
            ctx,
            call_result,
            tx_metadata,
        )))
    }
}

#[sdk_derive(MethodHandler)]
impl<Cfg: Config> Module<Cfg> {
    #[handler(call = "evm.Create")]
    fn tx_create<C: TxContext>(ctx: &mut C, body: types::Create) -> Result<Vec<u8>, Error> {
        Self::create(ctx, body.value, body.init_code)
    }

    #[handler(call = "evm.Call")]
    fn tx_call<C: TxContext>(ctx: &mut C, body: types::Call) -> Result<Vec<u8>, Error> {

        let code = Self::get_code(ctx, body.address)?;

        // Cache transaction information at check time for use in subsequent split transactions
        if ctx.mode() == Mode::CheckTx {
            let key = ctx.get_tx().to_vec();

            let sender = Self::derive_caller(ctx)?.to_fixed_bytes();
            let receiver = body.address.clone().to_fixed_bytes();

            INFO_CACHE.lock().unwrap().put(key, (sender, receiver, code.is_empty()));
        }

        // GBNOTE: if to address returns no code, means this is an external account. Call transfer directly.
        // println!("gbtest tx_call of code: {:?}, value: {}, file: {}, line: {}", code, body.value, file!(), line!());
        if code.is_empty() {
            Self::transfer(ctx, body.address, body.value, body.data)
        } else {
            Self::call(ctx, body.address, body.value, body.data)
        }
    }

    #[handler(query = "evm.Storage")]
    fn query_storage<C: Context>(ctx: &mut C, body: types::StorageQuery) -> Result<Vec<u8>, Error> {
        Self::get_storage(ctx, body.address, body.index)
    }

    #[handler(query = "evm.Code")]
    fn query_code<C: Context>(ctx: &mut C, body: types::CodeQuery) -> Result<Vec<u8>, Error> {
        Self::get_code(ctx, body.address)
    }

    #[handler(query = "evm.Balance")]
    fn query_balance<C: Context>(ctx: &mut C, body: types::BalanceQuery) -> Result<u128, Error> {
        Self::get_balance(ctx, body.address)
    }

    #[handler(query = "evm.SimulateCall", expensive, allow_private_km)]
    fn query_simulate_call<C: Context>(
        ctx: &mut C,
        body: types::SimulateCallQuery,
    ) -> Result<Vec<u8>, Error> {
        let cfg: LocalConfig = ctx.local_config(MODULE_NAME).unwrap_or_default();
        if cfg.query_simulate_call_max_gas > 0 && body.gas_limit > cfg.query_simulate_call_max_gas {
            return Err(Error::SimulationTooExpensive(
                cfg.query_simulate_call_max_gas,
            ));
        }
        Self::simulate_call(ctx, body)
    }

    #[handler(message_result = CONSENSUS_WITHDRAW_HANDLER)]
    fn message_result_withdraw<C: Context>(
        ctx: &mut C,
        me: MessageEvent,
        context: ConsensusWithdrawContext,
    ) {
        if !me.is_success() {
            // Transfer in failed, emit deposit failed event.
            ctx.emit_event(_Event::Deposit {
                from: context.from,
                nonce: context.nonce,
                to: context.address,
                eth_to: context.eth_addr,
                amount: context.amount.clone(),
                error: Some(me.into()),
            });
            return;
        }

        // Update runtime state.
        //Accounts::mint(ctx, context.address, &context.amount).unwrap();

        let addr = H160::from_slice(&context.eth_addr);
        let amt = u128_to_h256(context.amount.amount());

        let _ = Self::call_sc_mint(ctx, &addr, &amt, false);

        // Emit deposit successful event.
        ctx.emit_event(_Event::Deposit {
            from: context.from,
            nonce: context.nonce,
            to: context.address,
            eth_to: context.eth_addr,
            amount: context.amount.clone(),
            error: None,
        });
    }

    #[handler(call = "withdraw.reserve")]
    fn withdraw_reserve<C: TxContext>(ctx: &mut C, body: CallParam) -> Result<Vec<u8>, Error> {
        Self::call_sc_burn(
            ctx,
            &H160::from_slice(&body.address),
            &u128_to_h256(body.value),
            true,
        )
    }

    #[handler(message_result = CONSENSUS_TRANSFER_HANDLER)]
    fn message_result_transfer<C: Context>(
        ctx: &mut C,
        me: MessageEvent,
        context: ConsensusTransferContext,
    ) {
        if !me.is_success() {
            // Transfer out failed, refund the balance.
            /*
            Accounts::transfer(
                ctx,
                *ADDRESS_PENDING_WITHDRAWAL,
                context.address,
                &context.amount,
            )
            .expect("should have enough balance");
            */
            let to = H160::from_slice(&context.eth_addr);
            let amt = u128_to_h256(context.amount.amount());
            let _ = Self::call_sc_mint(ctx, &to, &amt, true);

            // Emit withdraw failed event.
            ctx.emit_event(_Event::Withdraw {
                from: context.address,
                eth_from: context.eth_addr,
                nonce: context.nonce,
                to: context.to,
                amount: context.amount.clone(),
                error: Some(me.into()),
            });
            return;
        }

        // Burn the withdrawn tokens.
        /*
        Accounts::burn(ctx, *ADDRESS_PENDING_WITHDRAWAL, &context.amount)
            .expect("should have enough balance");
        */
        let addr = H160::from_str(DW_SYSTEM_ADDRESS).unwrap();
        let amt = u128_to_h256(context.amount.amount());
        let _ = Self::call_sc_burn(ctx, &addr, &amt, false);

        // Emit withdraw successful event.
        ctx.emit_event(_Event::Withdraw {
            from: context.address,
            eth_from: context.eth_addr,
            nonce: context.nonce,
            to: context.to,
            amount: context.amount.clone(),
            error: None,
        });
    }
}

impl<Cfg: Config> Module<Cfg> {
    /// Initialize state from genesis.
    fn init<C: Context>(ctx: &mut C, genesis: Genesis) {
        // Set genesis parameters.
        Self::set_params(ctx.runtime_state(), genesis.parameters);
    }

    /// Migrate state from a previous version.
    fn migrate<C: Context>(_ctx: &mut C, _from: u32) -> bool {
        // No migrations currently supported.
        false
    }
}

impl<Cfg: Config> module::MigrationHandler for Module<Cfg> {
    type Genesis = Genesis;

    fn init_or_migrate<C: Context>(
        ctx: &mut C,
        meta: &mut modules::core::types::Metadata,
        genesis: Self::Genesis,
    ) -> bool {
        let version = meta.versions.get(Self::NAME).copied().unwrap_or_default();
        if version == 0 {
            // Initialize state from genesis.
            Self::init(ctx, genesis);
            meta.versions.insert(Self::NAME.to_owned(), Self::VERSION);
            return true;
        }

        // Perform migration.
        Self::migrate(ctx, version)
    }
}

impl<Cfg: Config> module::TransactionHandler for Module<Cfg> {
    fn decode_tx<C: Context>(
        _ctx: &mut C,
        scheme: &str,
        body: &[u8],
    ) -> Result<Option<Transaction>, CoreError> {
        match scheme {
            "evm.ethereum.v0" => Ok(Some(
                raw_tx::decode(body, Some(Cfg::CHAIN_ID))
                    .map_err(CoreError::MalformedTransaction)?,
            )),
            _ => Ok(None),
        }
    }
}

impl<Cfg: Config> module::BlockHandler for Module<Cfg> {
    fn end_block<C: Context>(ctx: &mut C) {
        // Update the list of historic block hashes.
        let block_number = ctx.runtime_header().round;
        let block_hash = ctx.runtime_header().encoded_hash();
        let mut block_hashes = state::block_hashes(ctx.runtime_state());

        let current_number = block_number;
        block_hashes.insert(block_number.to_be_bytes(), block_hash);

        if current_number > state::BLOCK_HASH_WINDOW_SIZE {
            let start_number = current_number - state::BLOCK_HASH_WINDOW_SIZE;
            block_hashes.remove(start_number.to_be_bytes());
        }
    }
}

impl<Cfg: Config> module::InvariantHandler for Module<Cfg> {}
