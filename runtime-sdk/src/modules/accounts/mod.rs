//! Accounts module.
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::TryInto,
};

use num_traits::Zero;
use once_cell::sync::Lazy;
use thiserror::Error;
use strum::IntoEnumIterator;

use crate::{
    context::{Context, TxContext},
    core::common::quantity::Quantity,
    handler, module,
    module::{Module as _, Parameters as _},
    modules,
    modules::core::{Error as CoreError, API as _},
    runtime::Runtime,
    sdk_derive,
    sender::SenderMeta,
    storage,
    storage::Prefix,
    types::{
        address::{Address, SignatureAddressSpec},
        token,
        transaction::{AuthInfo, Transaction},
        role::{self, Role}, proposal::ProposalState,
        vote::{Action,Vote},
    },
};


#[cfg(test)]
pub(crate) mod test;
pub mod types;

/// Unique module name.
const MODULE_NAME: &str = "accounts";

/// Maximum delta that the transaction nonce can be in the future from the current nonce to still
/// be accepted during transaction checks.
const MAX_CHECK_NONCE_FUTURE_DELTA: u64 = 0; // Increase once supported in Oasis Core.

/// Errors emitted by the accounts module.
#[derive(Error, Debug, oasis_runtime_sdk_macros::Error)]
pub enum Error {
    #[error("invalid argument")]
    #[sdk_error(code = 1)]
    InvalidArgument,

    #[error("insufficient balance")]
    #[sdk_error(code = 2)]
    InsufficientBalance,

    #[error("forbidden by policy")]
    #[sdk_error(code = 3)]
    Forbidden,

    #[error("not found")]
    #[sdk_error(code = 4)]
    NotFound,

    // GB: defined for proposal.
    #[error("invalid role")]
    #[sdk_error(code = 5)]
    InvalidRole,

    #[error("invalid proposal state")]
    #[sdk_error(code = 6)]
    InvalidState,

    #[error("counter overflow")]
    #[sdk_error(code = 7)]
    CounterOverflow,

    #[error("core: {0}")]
    #[sdk_error(transparent)]
    Core(#[from] modules::core::Error),

    //sifei: for quorum
    #[error("invalid proposal quorum")]
    #[sdk_error(code = 8)]
    InvalidQuorum,

    //Sifei:for Role 
    #[error("invalid proposal role no")]
    #[sdk_error(code = 9)]
    InvalidRolesNo,

    //Sifei:for proposal verification 
    #[error("voted already")]
    #[sdk_error(code = 10)]
    VoteDup,

}


/// Events emitted by the accounts module.
#[derive(Debug, cbor::Encode, oasis_runtime_sdk_macros::Event)]
#[cbor(untagged)]
pub enum Event {
    #[sdk_event(code = 1)]
    Transfer {
        from: Address,
        to: Address,
        amount: token::BaseUnits,
        // GBTODO: stop here currently.
        // txseq: u128,
        // GBTODO: debug later when necessary.
        // txinfo: String,
    },

    #[sdk_event(code = 2)]
    Burn {
        owner: Address,
        amount: token::BaseUnits,
    },

    #[sdk_event(code = 3)]
    Mint {
        owner: Address,
        amount: token::BaseUnits,
    },
}

/// Gas costs.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct GasCosts {
    pub tx_transfer: u64,

    // GB: gas cost for all mint/burn/whitelist/blacklist/editrole etc manage stable coin.
    pub tx_managest: u64,
}

/// Parameters for the accounts module.
#[derive(Clone, Default, Debug, cbor::Encode, cbor::Decode)]
pub struct Parameters {
    pub transfers_disabled: bool,
    // GB: insert field to disable mint and burn.
    pub mintst_disabled: bool,
    pub burnst_disabled: bool,
    // GB: insert field for chain_initiator.
    pub chain_initiator: Address,


    pub gas_costs: GasCosts,

    #[cbor(optional)]
    pub debug_disable_nonce_check: bool,

    #[cbor(optional)]
    pub denomination_infos: BTreeMap<token::Denomination, types::DenominationInfo>,
}

/// Errors emitted during rewards parameter validation.
#[derive(Error, Debug)]
pub enum ParameterValidationError {
    #[error("debug option used: {0}")]
    DebugOptionUsed(String),
}

impl module::Parameters for Parameters {
    type Error = ParameterValidationError;

    #[cfg(not(feature = "unsafe-allow-debug"))]
    fn validate_basic(&self) -> Result<(), Self::Error> {
        if self.debug_disable_nonce_check {
            return Err(ParameterValidationError::DebugOptionUsed(
                "debug_disable_nonce_check".to_string(),
            ));
        }

        Ok(())
    }
}

/// Genesis state for the accounts module.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct Genesis {
    pub parameters: Parameters,
    pub accounts: BTreeMap<Address, types::Account>,
    pub balances: BTreeMap<Address, BTreeMap<token::Denomination, u128>>,
    pub total_supplies: BTreeMap<token::Denomination, u128>,
    // GB: can define roles to addresses initially.
    pub roles_accounts: BTreeMap<role::Role, Vec<Address>>,
}

/// Interface that can be called from other modules.
pub trait API {
    /// Transfer an amount from one account to the other.
    fn transfer<C: Context>(
        ctx: &mut C,
        from: Address,
        to: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), Error>;

    /// Mint new tokens, increasing the total supply.
    fn mint<C: Context>(ctx: &mut C, to: Address, amount: &token::BaseUnits) -> Result<(), Error>;

    /// Burn existing tokens, decreasing the total supply.
    fn burn<C: Context>(ctx: &mut C, from: Address, amount: &token::BaseUnits)
        -> Result<(), Error>;

    /// Sets an account's nonce.
    fn set_nonce<S: storage::Store>(state: S, address: Address, nonce: u64);

    /// Fetch an account's current nonce.
    fn get_nonce<S: storage::Store>(state: S, address: Address) -> Result<u64, Error>;

    fn get_proposal_id<S: storage::Store>(state: S) -> Result<u32, Error>;
    fn get_proposal<S: storage::Store>(state: S, id: u32) -> Result<types::Proposal, Error>;
    fn get_and_increment_proposal_id<S: storage::Store>(state: S) -> Result<u32, Error>;
    fn insert_proposal<S: storage::Store>(state: S, proposal: types::Proposal) -> Result<(), Error>;

    fn get_voter_with_action(action: Action) -> Option<Role>;
    fn get_proposer_with_action(action: Action) -> Option<Role>;
    //Sifei: added for quorum, role counter
    fn get_quorum<S: storage::Store>(state: S, action:Action) -> Result<u8, Error>;
    fn set_quorum<S: storage::Store>(state: S, action:Action, quorum: u8) -> Result<(), Error>;
    fn get_voters_num_with_action<S: storage::Store>(state: S, action: Action) -> Result<u16, Error>;

    fn add_address_to_roles<S: storage::Store>(state: S, address: Address, role: role::Role) -> Result<(), Error>;

    fn add_role_to_address<S: storage::Store>(state: S, address: Address, role: role::Role);
    fn get_addrsno_in_role<S: storage::Store>(state: S, role: role::Role) -> u16;
    fn get_addresses_in_role<S: storage::Store>(state: S, role: role::Role) -> Result<Vec<Address>, Error>;


    fn set_role<S: storage::Store>(state: S, address: Address, role: role::Role);
    fn get_role<S: storage::Store>(state: S, address: Address) -> Result<role::Role, Error>;
    fn set_initstatus<S: storage::Store>(state: S, address: Address, init: bool);
    fn get_initstatus<S: storage::Store>(state: S, address: Address) -> Result<bool, Error>;

    /// Sets an account's balance of the given denomination.
    ///
    /// # Warning
    ///
    /// This method is dangerous as it can result in invariant violations.
    fn set_balance<S: storage::Store>(state: S, address: Address, amount: &token::BaseUnits);

    /// Fetch an account's balance of the given denomination.
    fn get_balance<S: storage::Store>(
        state: S,
        address: Address,
        denomination: token::Denomination,
    ) -> Result<u128, Error>;

    /// Ensures that the given account has at least the specified balance.
    fn ensure_balance<S: storage::Store>(
        state: S,
        address: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), Error> {
        let balance = Self::get_balance(state, address, amount.denomination().clone())?;
        if balance < amount.amount() {
            Err(Error::InsufficientBalance)
        } else {
            Ok(())
        }
    }

    /// Fetch an account's current balances.
    fn get_balances<S: storage::Store>(
        state: S,
        address: Address,
    ) -> Result<types::AccountBalances, Error>;

    /// Fetch addresses.
    fn get_addresses<S: storage::Store>(
        state: S,
        denomination: token::Denomination,
    ) -> Result<Vec<Address>, Error>;

    /// Fetch total supplies.
    fn get_total_supplies<S: storage::Store>(
        state: S,
    ) -> Result<BTreeMap<token::Denomination, u128>, Error>;

    /// Sets the total supply for the given denomination.
    ///
    /// # Warning
    ///
    /// This method is dangerous as it can result in invariant violations.
    fn set_total_supply<S: storage::Store>(state: S, amount: &token::BaseUnits);

    /// Fetch information about a denomination.
    fn get_denomination_info<S: storage::Store>(
        state: S,
        denomination: &token::Denomination,
    ) -> Result<types::DenominationInfo, Error>;

    /// Move amount from address into fee accumulator.
    fn move_into_fee_accumulator<C: Context>(
        ctx: &mut C,
        from: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), modules::core::Error>;

    /// Move amount from fee accumulator into address.
    fn move_from_fee_accumulator<C: Context>(
        ctx: &mut C,
        to: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), modules::core::Error>;

    /// Check transaction signer account nonces.
    /// Return payer address.
    fn check_signer_nonces<C: Context>(
        ctx: &mut C,
        tx_auth_info: &AuthInfo,
    ) -> Result<Address, modules::core::Error>;

    /// Update transaction signer account nonces.
    fn update_signer_nonces<C: Context>(
        ctx: &mut C,
        tx_auth_info: &AuthInfo,
    ) -> Result<(), modules::core::Error>;
}

/// State schema constants.
pub mod state {
    /// Map of account addresses to account metadata.
    pub const ACCOUNTS: &[u8] = &[0x01];
    /// Map of account addresses to map of denominations to balances.
    pub const BALANCES: &[u8] = &[0x02];
    /// Map of total supplies (per denomination).
    pub const TOTAL_SUPPLY: &[u8] = &[0x03];

    /// sifei: Map of roles to addresses, may put into PROPOSALS state directly instead of individual storage.
    pub const ROLES: &[u8] = &[0x04];
    /// Map of proposal id to addresses.
    pub const PROPOSALS: &[u8] = &[0x05];
}


pub struct Module;

/// Module's address that has the common pool.
pub static ADDRESS_COMMON_POOL: Lazy<Address> =
    Lazy::new(|| Address::from_bech32("hela01qqgthu582dkvkjxnhusg9gt8dh69jy0hfyt78p36").unwrap());
/// Module's address that has the fee accumulator.
pub static ADDRESS_FEE_ACCUMULATOR: Lazy<Address> =
    Lazy::new(|| Address::from_module(MODULE_NAME, "fee-accumulator"));

/// This is needed to properly iterate over the BALANCES map.
#[derive(Clone, PartialEq, PartialOrd, Eq, Ord)]
struct AddressWithDenomination(Address, token::Denomination);

#[derive(Error, Debug)]
enum AWDError {
    #[error("malformed address")]
    MalformedAddress,

    #[error("malformed denomination")]
    MalformedDenomination,
}

impl std::convert::TryFrom<&[u8]> for AddressWithDenomination {
    type Error = AWDError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let address =
            Address::try_from(&bytes[..Address::SIZE]).map_err(|_| AWDError::MalformedAddress)?;
        let denomination = token::Denomination::try_from(&bytes[Address::SIZE..])
            .map_err(|_| AWDError::MalformedDenomination)?;
        Ok(AddressWithDenomination(address, denomination))
    }
}

//Sifei: follow balances method to save Address and Role ( raw u8 format)
#[derive(Clone, PartialEq, PartialOrd, Eq, Ord)]
struct AddressWithRole(Address, [u8;Role::ROLE_SIZE]);

#[derive(Error, Debug)]
enum RAError {
    #[error("malformed address")]
    MalformedAddress,

    #[error("malformed role")]
    MalformedRole,
}

impl std::convert::TryFrom<&[u8]> for AddressWithRole{
    type Error = RAError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let address =
            Address::try_from(&bytes[..Address::SIZE]).map_err(|_| RAError::MalformedAddress)?;
        let role = Role::try_from(&bytes[Address::SIZE..])
            .map_err(|_| RAError::MalformedRole)?;
        Ok(AddressWithRole(address, role.marshal_binary()))
    }
}

impl Module {
    /// Add given amount of tokens to the specified account's balance.
    fn add_amount<S: storage::Store>(
        state: S,
        addr: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let balances = storage::PrefixStore::new(store, &state::BALANCES);
        let mut account = storage::TypedStore::new(storage::PrefixStore::new(balances, &addr));
        let mut value: u128 = account.get(amount.denomination()).unwrap_or_default();

        value = value
            .checked_add(amount.amount())
            .ok_or(Error::InvalidArgument)?;
        account.insert(amount.denomination(), value);
        Ok(())
    }

    /// Subtract given amount of tokens from the specified account's balance.
    fn sub_amount<S: storage::Store>(
        state: S,
        addr: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let balances = storage::PrefixStore::new(store, &state::BALANCES);
        let mut account = storage::TypedStore::new(storage::PrefixStore::new(balances, &addr));
        let mut value: u128 = account.get(amount.denomination()).unwrap_or_default();

        value = value
            .checked_sub(amount.amount())
            .ok_or(Error::InsufficientBalance)?;
        account.insert(amount.denomination(), value);
        Ok(())
    }

    /// Increment the total supply for the given amount.
    fn inc_total_supply<S: storage::Store>(
        state: S,
        amount: &token::BaseUnits,
    ) -> Result<(), Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut total_supplies =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::TOTAL_SUPPLY));
        let mut total_supply: u128 = total_supplies
            .get(amount.denomination())
            .unwrap_or_default();

        total_supply = total_supply
            .checked_add(amount.amount())
            .ok_or(Error::InvalidArgument)?;
        total_supplies.insert(amount.denomination(), total_supply);
        Ok(())
    }

    /// Decrement the total supply for the given amount.
    fn dec_total_supply<S: storage::Store>(
        state: S,
        amount: &token::BaseUnits,
    ) -> Result<(), Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut total_supplies =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::TOTAL_SUPPLY));
        let mut total_supply: u128 = total_supplies
            .get(amount.denomination())
            .unwrap_or_default();

        total_supply = total_supply
            .checked_sub(amount.amount())
            .ok_or(Error::InsufficientBalance)?;
        total_supplies.insert(amount.denomination(), total_supply);
        Ok(())
    }

    /// Get all balances.
    fn get_all_balances<S: storage::Store>(
        state: S,
    ) -> Result<BTreeMap<Address, BTreeMap<token::Denomination, u128>>, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let balances = storage::TypedStore::new(storage::PrefixStore::new(store, &state::BALANCES));

        // Unfortunately, we can't just return balances.iter().collect() here,
        // because the stored format doesn't match -- we need this workaround
        // instead.

        let balmap: BTreeMap<AddressWithDenomination, u128> = balances.iter().collect();

        let mut b: BTreeMap<Address, BTreeMap<token::Denomination, u128>> = BTreeMap::new();

        for (addrden, amt) in &balmap {
            let addr = &addrden.0;
            let den = &addrden.1;

            // Fetch existing account's balances or insert blank ones.
            let addr_bals = b.entry(*addr).or_insert_with(BTreeMap::new);

            // Add to given denomination's balance or insert it if new.
            addr_bals
                .entry(den.clone())
                .and_modify(|a| *a += amt)
                .or_insert_with(|| *amt);
        }

        Ok(b)
    }
}

/// A fee accumulator that stores fees from all transactions in a block.
#[derive(Default)]
pub struct FeeAccumulator {
    pub total_fees: BTreeMap<token::Denomination, u128>,
}

impl FeeAccumulator {
    /// Add given fee to the accumulator.
    pub fn add(&mut self, fee: &token::BaseUnits) {
        let current = self
            .total_fees
            .entry(fee.denomination().clone())
            .or_default();

        *current = current.checked_add(fee.amount()).unwrap(); // Should never overflow.
    }

    /// Subtract given fee from the accumulator.
    fn sub(&mut self, fee: &token::BaseUnits) -> Result<(), Error> {
        let current = self
            .total_fees
            .entry(fee.denomination().clone())
            .or_default();

        *current = current
            .checked_sub(fee.amount())
            .ok_or(Error::InsufficientBalance)?;
        Ok(())
    }
}

/// Context key for the fee accumulator.
pub const CONTEXT_KEY_FEE_ACCUMULATOR: &str = "accounts.FeeAccumulator";

impl API for Module {
    fn transfer<C: Context>(
        ctx: &mut C,
        from: Address,
        to: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), Error> {
        if ctx.is_check_only() {
            return Ok(());
        }

        // Subtract from source account.
        Self::sub_amount(ctx.runtime_state(), from, amount)?;
        // Add to destination account.
        Self::add_amount(ctx.runtime_state(), to, amount)?;

        // Emit a transfer event.
        ctx.emit_event(Event::Transfer {
            from,
            to,
            amount: amount.clone(),
            // GB: insert information for transfer/mint/burn later if necessary.
            // txseq: 1234567890,
            // txinfo: "testinfo".to_string(),
        });

        Ok(())
    }

    fn mint<C: Context>(ctx: &mut C, to: Address, amount: &token::BaseUnits) -> Result<(), Error> {
        // Add to destination account.
        Self::add_amount(ctx.runtime_state(), to, amount)?;

        // Increase total supply.
        Self::inc_total_supply(ctx.runtime_state(), amount)?;

        // Emit a mint event.
        ctx.emit_event(Event::Mint {
            owner: to,
            amount: amount.clone(),
        });

        Ok(())
    }

    fn burn<C: Context>(
        ctx: &mut C,
        from: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), Error> {
        // Remove from target account.
        Self::sub_amount(ctx.runtime_state(), from, amount)?;

        // Decrease total supply.
        Self::dec_total_supply(ctx.runtime_state(), amount)
            .expect("target account had enough balance so total supply should not underflow");

        // Emit a burn event.
        ctx.emit_event(Event::Burn {
            owner: from,
            amount: amount.clone(),
        });

        Ok(())
    }

    fn set_nonce<S: storage::Store>(state: S, address: Address, nonce: u64) {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut accounts =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::ACCOUNTS));
        let mut account: types::Account = accounts.get(address).unwrap_or_default();
        account.nonce = nonce;
        accounts.insert(&address, account);
    }

    fn get_nonce<S: storage::Store>(state: S, address: Address) -> Result<u64, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let accounts = storage::TypedStore::new(storage::PrefixStore::new(store, &state::ACCOUNTS));
        let account: types::Account = accounts.get(address).unwrap_or_default();
        Ok(account.nonce)
    }

    fn get_proposal_id<S: storage::Store>(state: S) -> Result<u32, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let proposals =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::PROPOSALS));

        // sifei: please refer this proposal_id to keep quorums to the state.
        const PROPOSAL_COUNTER_KEY: &[u8] = b"proposal_id";
        let counter: u32 = proposals.get(PROPOSAL_COUNTER_KEY).unwrap_or(0);
        Ok(counter)
    }

    fn get_proposal<S: storage::Store>(state: S, id: u32) -> Result<types::Proposal, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let proposals =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::PROPOSALS));

        let proposal_id_bytes = id.to_le_bytes();
        let proposal: types::Proposal = proposals.get(proposal_id_bytes).unwrap_or_default();

        Ok(proposal)
    }


    fn get_and_increment_proposal_id<S: storage::Store>(state: S) -> Result<u32, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut proposals =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::PROPOSALS));

        const PROPOSAL_COUNTER_KEY: &[u8] = b"proposal_id";
        let mut counter: u32 = proposals.get(PROPOSAL_COUNTER_KEY).unwrap_or(0);
        counter = counter.checked_add(1).ok_or(Error::CounterOverflow)?;
        proposals.insert(PROPOSAL_COUNTER_KEY, counter);

        Ok(counter)
    }


    fn insert_proposal<S: storage::Store>(state: S, proposal: types::Proposal) -> Result<(), Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut proposals =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::PROPOSALS));

        let proposal_id_bytes = proposal.id.to_le_bytes();
        proposals.insert(proposal_id_bytes, proposal);

        Ok(())
    }


    fn get_voter_with_action(action: Action) -> Option<Role> {
        match action {
            Action::NoAction => None,
            Action::SetRoles => Some(Role::Admin),
            Action::Mint => Some(Role::MintVoter),
            Action::Burn => Some(Role::BurnVoter),
            Action::Whitelist => Some(Role::WhitelistVoter),
            Action::Blacklist => Some(Role::BlacklistVoter),
            Action::Config => Some(Role::Admin),
        }
    }

    fn get_proposer_with_action(action: Action) -> Option<Role> {
        match action {
            Action::NoAction => None,
            Action::SetRoles => Some(Role::Admin),
            Action::Mint => Some(Role::MintProposer),
            Action::Burn => Some(Role::BurnProposer),
            Action::Whitelist => Some(Role::WhitelistProposer),
            Action::Blacklist => Some(Role::BlacklistProposer),
            Action::Config => Some(Role::Admin),
        }
    }

    //Sifei: get_quorum for Burn/Mint/Whitelist/Blacklist/Config
    fn get_quorum<S: storage::Store>(state: S, action: Action) -> Result<u8, Error>  {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let proposals =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::PROPOSALS));
 
        const PROPOSAL_MINT_KEY:  &[u8] = b"proposal_mint_quorum";
        const PROPOSAL_BURN_KEY:  &[u8] = b"proposal_burn_quorum";
        const PROPOSAL_WHITELIST_KEY:  &[u8] = b"proposal_whitelist_quorum";
        const PROPOSAL_BLACKLIST_KEY:  &[u8] = b"proposal_blacklist_quorum";
        const PROPOSAL_CONFIG_KEY:  &[u8] = b"proposal_config_quorum";

        // sifei: get quorum
        let quorum: u8 = match action {
            Action::Mint => proposals.get(PROPOSAL_MINT_KEY).unwrap_or(100),
            Action::Burn => proposals.get(PROPOSAL_BURN_KEY).unwrap_or(100),
            Action::Whitelist => proposals.get(PROPOSAL_WHITELIST_KEY).unwrap_or(100),
            Action::Blacklist => proposals.get(PROPOSAL_BLACKLIST_KEY).unwrap_or(100),
            Action::Config => proposals.get(PROPOSAL_CONFIG_KEY).unwrap_or(100),
            Action::SetRoles => proposals.get(PROPOSAL_CONFIG_KEY).unwrap_or(100),
            _ => return Err(Error::NotFound),
        };
        Ok(quorum)
    }

    //Sifei: set_quorum for Burn/Mint/Whitelist/Blacklist/Config
    fn set_quorum<S: storage::Store>(state: S, action: Action, quorum:u8) -> Result<(), Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut proposals =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::PROPOSALS));
        const PROPOSAL_MINT_KEY:  &[u8] = b"proposal_mint_quorum";
        const PROPOSAL_BURN_KEY:  &[u8] = b"proposal_burn_quorum";
        const PROPOSAL_WHITELIST_KEY:  &[u8] = b"proposal_whitelist_quorum";
        const PROPOSAL_BLACKLIST_KEY:  &[u8] = b"proposal_blacklist_quorum";
        const PROPOSAL_CONFIG_KEY:  &[u8] = b"proposal_config_quorum";

        match action {
            Action::Mint => proposals.insert(PROPOSAL_MINT_KEY, quorum),
            Action::Burn => proposals.insert(PROPOSAL_BURN_KEY, quorum),
            Action::Whitelist => proposals.insert(PROPOSAL_WHITELIST_KEY, quorum),
            Action::Blacklist => proposals.insert(PROPOSAL_BLACKLIST_KEY, quorum),
            Action::Config => proposals.insert(PROPOSAL_CONFIG_KEY, quorum),
            _ => return Err(Error::NotFound),
        };
        Ok(())
    }

    fn set_role<S: storage::Store>(state: S, address: Address, role: role::Role) {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut accounts =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::ACCOUNTS));
        let mut account: types::Account = accounts.get(address).unwrap_or_default();
        account.role = role;
        accounts.insert(&address, account);
    }



    /// GB: add an address to some role (e.g. MintVoter), so this role holds all addresses of such a role.
    /// this function should be used together with remove_address_from_roles, to maintain the sequ8 counter.
    fn add_address_to_roles<S: storage::Store>(state: S, address: Address, role: role::Role) -> Result<(), Error> {
        // GB: the following to insert the address to the corresponding role vec.
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let roles_store = storage::PrefixStore::new(store, &state::ROLES);
        let role_bytes = role.marshal_binary();
        let mut role_store =
             storage::TypedStore::new(storage::PrefixStore::new(roles_store, &role_bytes));

        // GB: define a map to save the sequence of specific role's addresses.
        let role_str = role.to_string();
        let seq_key = "seq".to_string() + &role_str;
        let seq_key_bytes = seq_key.as_bytes();
        let mut counter: u32 = role_store.get(&seq_key_bytes).unwrap_or(0);
        counter = counter.checked_add(1).ok_or(Error::CounterOverflow)?;
        role_store.insert(&seq_key_bytes, counter);

        // GBTODO: decrease the counter while removing an address from a role.
        role_store.insert(&address, counter);

        Ok(())
    }

    //Sifei: save address with role to store
    /// GB: map one address to multi roles, but we only allow one role for an address at current stage.
    /// we remove the address from ROLES storage while new role comes to avoid inconsistency.
    fn add_role_to_address<S: storage::Store>(state: S, address: Address, role: role::Role) {
        // GB: the following to insert the address to the corresponding role vec.
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let roles_store = storage::PrefixStore::new(store, &state::ROLES);

        let mut role_account =
             storage::TypedStore::new(storage::PrefixStore::new(roles_store, &address));

        // GB: remove this address's storage (all role->bool mappings) first.
        // this is just a workaround, still need to save a lot of address even after the users are set to User.
        for role in Role::iter() {
            let rawu8role = role.marshal_binary();
            role_account.remove(&rawu8role);
        }

        if role != Role::User {
            // Update the map in the store.
            let rawu8 = role.marshal_binary();
            role_account.insert(rawu8, true);
        }
    }


    //Sifei: get no of addresses with requested role
    fn get_addrsno_in_role<S: storage::Store>(state: S, role: role::Role) -> u16 {
        //Option variable
        let rstaddresses = Self::get_addresses_in_role (state, role);
        let addressno = match rstaddresses {
            Ok(addresses) => addresses.len(),
            Err(_) => 0,
        };
        addressno as u16
    }

    //Sifei: get addresses with requested role
    /*    
    GB: get all addresses of some role by iterating all the addresses in the ROLES storage, 
    it would become quite cubersome when a lot of addresses are handled.
    */
    fn get_addresses_in_role<S: storage::Store>(state: S, role: role::Role) -> Result<Vec<Address>, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let role_addresses: BTreeMap<AddressWithRole, bool> =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::ROLES))
            .iter()
            .collect();

        //get addresses 
        Ok(role_addresses
            .into_keys()
            .filter(|ra| ra.1 == role.marshal_binary())
            .map(|ra| ra.0)
            .collect())
    }

    //Sifei: get no of voters for action
    fn get_voters_num_with_action<S: storage::Store>(state: S, action: Action) -> Result<u16, Error> {
        let  voters= match action {
              Action::Mint => Self::get_addrsno_in_role(state, role::Role::MintVoter),
              Action::Burn => Self::get_addrsno_in_role(state, role::Role::BurnVoter),
              Action::Whitelist => Self::get_addrsno_in_role(state, role::Role::WhitelistVoter),
              Action::Blacklist => Self::get_addrsno_in_role(state, role::Role::BlacklistVoter),
              Action::Config => Self::get_addrsno_in_role(state, role::Role::Admin),
              Action::SetRoles=> Self::get_addrsno_in_role(state, role::Role::Admin),
              Action::NoAction=> return Err(Error::NotFound),
        };
        Ok(voters as u16)
    }


    fn get_role<S: storage::Store>(state: S, address: Address) -> Result<role::Role, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let accounts = storage::TypedStore::new(storage::PrefixStore::new(store, &state::ACCOUNTS));
        let account: types::Account = accounts.get(address).unwrap_or_default();
        Ok(account.role)
    }

    fn set_initstatus<S: storage::Store>(state: S, address: Address, init: bool) {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut accounts =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::ACCOUNTS));
        let mut account: types::Account = accounts.get(address).unwrap_or_default();
        account.init = init;
        accounts.insert(&address, account);
    }

    fn get_initstatus<S: storage::Store>(state: S, address: Address) -> Result<bool, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let accounts = storage::TypedStore::new(storage::PrefixStore::new(store, &state::ACCOUNTS));
        let account: types::Account = accounts.get(address).unwrap_or_default();
        Ok(account.init)
    }


    fn set_balance<S: storage::Store>(state: S, address: Address, amount: &token::BaseUnits) {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let balances = storage::PrefixStore::new(store, &state::BALANCES);
        let mut account = storage::TypedStore::new(storage::PrefixStore::new(balances, &address));
        account.insert(amount.denomination(), amount.amount());
    }

    fn get_balance<S: storage::Store>(
        state: S,
        address: Address,
        denomination: token::Denomination,
    ) -> Result<u128, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let balances = storage::PrefixStore::new(store, &state::BALANCES);
        let account = storage::TypedStore::new(storage::PrefixStore::new(balances, &address));

        Ok(account.get(&denomination).unwrap_or_default())
    }

    fn get_balances<S: storage::Store>(
        state: S,
        address: Address,
    ) -> Result<types::AccountBalances, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let balances = storage::PrefixStore::new(store, &state::BALANCES);
        let account = storage::TypedStore::new(storage::PrefixStore::new(balances, &address));

        Ok(types::AccountBalances {
            balances: account.iter().collect(),
        })
    }

    fn get_addresses<S: storage::Store>(
        state: S,
        denomination: token::Denomination,
    ) -> Result<Vec<Address>, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let balances: BTreeMap<AddressWithDenomination, Quantity> =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::BALANCES))
                .iter()
                .collect();

        Ok(balances
            .into_keys()
            .filter(|bal| bal.1 == denomination)
            .map(|bal| bal.0)
            .collect())
    }

    fn get_total_supplies<S: storage::Store>(
        state: S,
    ) -> Result<BTreeMap<token::Denomination, u128>, Error> {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let ts = storage::TypedStore::new(storage::PrefixStore::new(store, &state::TOTAL_SUPPLY));

        Ok(ts.iter().collect())
    }

    fn set_total_supply<S: storage::Store>(state: S, amount: &token::BaseUnits) {
        let store = storage::PrefixStore::new(state, &MODULE_NAME);
        let mut total_supplies =
            storage::TypedStore::new(storage::PrefixStore::new(store, &state::TOTAL_SUPPLY));
        total_supplies.insert(amount.denomination(), amount.amount());
    }

    fn get_denomination_info<S: storage::Store>(
        state: S,
        denomination: &token::Denomination,
    ) -> Result<types::DenominationInfo, Error> {
        let params = Self::params(state);
        params
            .denomination_infos
            .get(denomination)
            .cloned()
            .ok_or(Error::NotFound)
    }

    fn move_into_fee_accumulator<C: Context>(
        ctx: &mut C,
        from: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), modules::core::Error> {
        if ctx.is_simulation() {
            return Ok(());
        }

        Self::sub_amount(ctx.runtime_state(), from, amount)
            .map_err(|_| modules::core::Error::InsufficientFeeBalance)?;

        ctx.value::<FeeAccumulator>(CONTEXT_KEY_FEE_ACCUMULATOR)
            .or_default()
            .add(amount);

        Ok(())
    }

    fn move_from_fee_accumulator<C: Context>(
        ctx: &mut C,
        to: Address,
        amount: &token::BaseUnits,
    ) -> Result<(), modules::core::Error> {
        if ctx.is_simulation() {
            return Ok(());
        }

        ctx.value::<FeeAccumulator>(CONTEXT_KEY_FEE_ACCUMULATOR)
            .or_default()
            .sub(amount)
            .map_err(|_| modules::core::Error::InsufficientFeeBalance)?;

        Self::add_amount(ctx.runtime_state(), to, amount)
            .map_err(|_| modules::core::Error::InsufficientFeeBalance)?;

        Ok(())
    }

    fn check_signer_nonces<C: Context>(
        ctx: &mut C,
        auth_info: &AuthInfo,
    ) -> Result<Address, modules::core::Error> {
        let is_pre_schedule = ctx.is_pre_schedule();
        let is_check_only = ctx.is_check_only();

        // TODO: Optimize the check/update pair so that the accounts are
        // fetched only once.
        let params = Self::params(ctx.runtime_state());
        // Fetch information about each signer.
        let mut store = storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let accounts =
            storage::TypedStore::new(storage::PrefixStore::new(&mut store, &state::ACCOUNTS));
        let mut sender = None;
        for si in auth_info.signer_info.iter() {
            let address = si.address_spec.address();
            let account: types::Account = accounts.get(address).unwrap_or_default();

            // First signer pays for the fees and is considered the sender.
            if sender.is_none() {
                sender = Some(SenderMeta {
                    address,
                    tx_nonce: si.nonce,
                    state_nonce: account.nonce,
                });
            }

            // When nonce checking is disabled, skip the rest of the checks.
            if params.debug_disable_nonce_check {
                continue;
            }

            // Check signer nonce against the corresponding account nonce.
            match si.nonce.cmp(&account.nonce) {
                Ordering::Less => {
                    // In the past and will never become valid, reject.
                    return Err(modules::core::Error::InvalidNonce);
                }
                Ordering::Equal => {} // Ok.
                Ordering::Greater => {
                    // If too much in the future, reject.
                    if si.nonce - account.nonce > MAX_CHECK_NONCE_FUTURE_DELTA {
                        return Err(modules::core::Error::InvalidNonce);
                    }

                    // If in the future and this is before scheduling, reject with separate error
                    // that will make the scheduler skip the transaction.
                    if is_pre_schedule {
                        return Err(modules::core::Error::FutureNonce);
                    }

                    // If in the future and this is during execution, reject.
                    if !is_check_only {
                        return Err(modules::core::Error::InvalidNonce);
                    }

                    // If in the future and this is during checks, accept.
                }
            }
        }

        // Configure the sender.
        let sender = sender.expect("at least one signer is always present");
        let sender_address = sender.address;
        if is_check_only {
            <C::Runtime as Runtime>::Core::set_sender_meta(ctx, sender);
        }

        Ok(sender_address)
    }

    fn update_signer_nonces<C: Context>(
        ctx: &mut C,
        auth_info: &AuthInfo,
    ) -> Result<(), modules::core::Error> {
        // Fetch information about each signer.
        let mut store = storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let mut accounts =
            storage::TypedStore::new(storage::PrefixStore::new(&mut store, &state::ACCOUNTS));
        for si in auth_info.signer_info.iter() {
            let address = si.address_spec.address();
            let mut account: types::Account = accounts.get(address).unwrap_or_default();

            // Update nonce.
            account.nonce = account
                .nonce
                .checked_add(1)
                .ok_or(modules::core::Error::InvalidNonce)?; // Should never overflow.
            accounts.insert(&address, account);
        }
        Ok(())
    }
}

#[sdk_derive(MethodHandler)]
impl Module {
    #[handler(prefetch = "accounts.Transfer")]
    fn prefetch_transfer(
        add_prefix: &mut dyn FnMut(Prefix),
        body: cbor::Value,
        auth_info: &AuthInfo,
    ) -> Result<(), crate::error::RuntimeError> {
        let args: types::Transfer = cbor::from_value(body).map_err(|_| Error::InvalidArgument)?;
        let from = auth_info.signer_info[0].address_spec.address();

        // Prefetch accounts 'to'.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, args.to.as_ref()].concat(),
        ));
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::BALANCES, args.to.as_ref()].concat(),
        ));
        // Prefetch accounts 'from'.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, from.as_ref()].concat(),
        ));
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::BALANCES, from.as_ref()].concat(),
        ));

        Ok(())
    }

    #[handler(call = "accounts.Transfer")]
    fn tx_transfer<C: TxContext>(ctx: &mut C, body: types::Transfer) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());

        // Reject transfers when they are disabled.
        if params.transfers_disabled {
            return Err(Error::Forbidden);
        }

        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, params.gas_costs.tx_transfer)?;

        Self::transfer(ctx, ctx.tx_caller_address(), body.to, &body.amount)?;

        Ok(())
    }



/*####################################################################################################*/
    #[handler(prefetch = "accounts.Propose")]
    fn prefetch_propose(
        add_prefix: &mut dyn FnMut(Prefix),
        _body: cbor::Value,
        auth_info: &AuthInfo,
    ) -> Result<(), crate::error::RuntimeError> {
        // let args: types::ProposalContent = cbor::from_value(body).map_err(|_| Error::InvalidArgument)?;
        let from = auth_info.signer_info[0].address_spec.address();

        // GB: prefetch the transaction origin.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, from.as_ref()].concat(),
        ));

        // GB: prefetch the proposal id
        // const PROPOSAL_COUNTER_KEY: &[u8] = b"proposal_id";
        // add_prefix(Prefix::from(
        //     [MODULE_NAME.as_bytes(), state::PROPOSALS, PROPOSAL_COUNTER_KEY].concat(),
        // ));

        Ok(())
    }

    #[handler(call = "accounts.Propose")]
    fn tx_propose<C: TxContext>(ctx: &mut C, body: types::ProposalContent) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());
        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, params.gas_costs.tx_managest)?;

        let caller_address = ctx.tx_caller_address();
        let caller_role = Self::get_role(ctx.runtime_state(), caller_address).unwrap_or_default();

        // println!("gbtest: caller_address is {:?}: ", caller_address);
        // GBTODO: add more guards, like the previous proposal must finish before a new proposal;
        // the proposal period is ? etc.

        // the proposal id starts from 1.
        let next_id = Self::get_and_increment_proposal_id(ctx.runtime_state())?;
        let proposalcontent = body.clone();

        // GB: only the correct Proposers and Admin can propose something.
        // GBTODO: the correct voters can also propose.
        let proposer_role = Self::get_proposer_with_action(proposalcontent.action);
        let voter_role = Self::get_voter_with_action(proposalcontent.action);

        let mut is_proposer = false;
        let mut is_voter = false;

        if let Some(role) = proposer_role {
            if caller_role == role {
                is_proposer = true;
            }
        } else {
            return Err(Error::InvalidRole);
        }

        if let Some(role) = voter_role {
            if caller_role == role {
                is_voter = true;
            }
        } else {
            return Err(Error::InvalidRole);
        }

        if !(is_proposer || is_voter) {            
            return Err(Error::InvalidRole);
        }


        match proposalcontent.action {
            // GB: both Mint/Burn action must operate on the WhitelistedUser.
            Action::Mint | Action::Burn => {
                let address = match proposalcontent.data.address {
                    None  =>  return Err(Error::NotFound),
                    Some(addr) => addr,
                };

                let addr_role = Self::get_role(ctx.runtime_state(), address).unwrap_or_default();
                if addr_role != Role::WhitelistedUser {
                    return Err(Error::InvalidArgument);
                }
            },

            // GB: no constraints for SetRoles, admin can change any roles.
            Action::SetRoles => {},

            // GB: quorum for config should be [0, 100], and there is a least one quorum in this proposal.
            Action::Config => {
                let data = &proposalcontent.data;
                let is_valid = |quorum: &Option<u8>| quorum.map_or(true, |value| value <= 100);
                let is_some = |quorum: &Option<u8>| quorum.is_some();

                let valid_values = is_valid(&data.mint_quorum) &&
                is_valid(&data.burn_quorum) &&
                is_valid(&data.whitelist_quorum) &&
                is_valid(&data.blacklist_quorum) &&
                is_valid(&data.config_quorum);

                let at_least_one_some = is_some(&data.mint_quorum) ||
                is_some(&data.burn_quorum) ||
                is_some(&data.whitelist_quorum) ||
                is_some(&data.blacklist_quorum) ||
                is_some(&data.config_quorum);

                if !(valid_values && at_least_one_some){
                    return Err(Error::InvalidArgument);
                }
            },

            /*
            GB: Whitelist action can operate on all roles, except BlacklistedUser, 
            otherwise, the admin must make the BlacklistedUser into User role, then add into Whitelist.
            */
            Action::Whitelist => {
                let address = match proposalcontent.data.address {
                    None  =>  return Err(Error::NotFound),
                    Some(addr) => addr,
                };

                let addr_role = Self::get_role(ctx.runtime_state(), address).unwrap_or_default();
                if addr_role == Role::BlacklistedUser {
                    return Err(Error::InvalidArgument);
                }
            },  

            /*                
            GB: blacklist action can only operate on normal User role, 
            otherwise, the admin must make the other roles into User role, then add into blacklist.
            */                      
            Action::Blacklist => {
                let address = match proposalcontent.data.address {
                    None  =>  return Err(Error::NotFound),
                    Some(addr) => addr,
                };

                let addr_role = Self::get_role(ctx.runtime_state(), address).unwrap_or_default();
                if addr_role != Role::User {
                    return Err(Error::InvalidArgument);
                }
            },

            _ => { return Err(Error::InvalidArgument); },
        }

        let proposal = types::Proposal {
            id: next_id,
            submitter: caller_address, // Use the submitter's address.
            state: ProposalState::Active,
            content: body,   
            results: None,
            voteOption: None,
        };

        Self::insert_proposal(ctx.runtime_state(), proposal)?;


        // println!("gbtest: insert_proposal.");
        Ok(())
    }


    #[handler(prefetch = "accounts.VoteST")]
    fn prefetch_votest(
        add_prefix: &mut dyn FnMut(Prefix),
        _body: cbor::Value,
        auth_info: &AuthInfo,
    ) -> Result<(), crate::error::RuntimeError> {
        // let args: types::ProposalContent = cbor::from_value(body).map_err(|_| Error::InvalidArgument)?;
        let from = auth_info.signer_info[0].address_spec.address();

        // GB: prefetch the transaction origin.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, from.as_ref()].concat(),
        ));

        // GB: prefetch the proposal id
        // const PROPOSAL_COUNTER_KEY: &[u8] = b"proposal_id";
        // add_prefix(Prefix::from(
        //     [MODULE_NAME.as_bytes(), state::PROPOSALS, PROPOSAL_COUNTER_KEY].concat(),
        // ));

        Ok(())
    }

    #[handler(call = "accounts.VoteST")]
    fn tx_votest<C: TxContext>(ctx: &mut C, body: types::VoteProposal) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());
        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, params.gas_costs.tx_managest)?;

        let caller_address = ctx.tx_caller_address();
        let caller_role = Self::get_role(ctx.runtime_state(), caller_address).unwrap_or_default();


        // println!("gbtest: caller_address is {:?}: ", caller_address);
        let mut proposal = Self::get_proposal(ctx.runtime_state(), body.id)?;
        // println!("gbtest file: {}, line: {}", file!(), line!());

        // check whether the caller has voted or not.
        let mut vote_option = proposal.voteOption;
        if let Some(map) = vote_option.as_mut() {
            if map.contains_key(&caller_address) {
                // println!("gbtest: The address '{}' is present in the map.", caller_address);
                return Err(Error::VoteDup);
            } else {
                // println!("gbtest: The address '{}' is not found in the map.", caller_address);
                map.insert(caller_address, body.option);
                proposal.voteOption = Some(map.clone());
            }
        } else {
            // println!("gbtest: The map is None.");
            let mut map = HashMap::new();
            map.insert(caller_address, body.option);
            proposal.voteOption = Some(map);
        }
        

        if proposal.state == ProposalState::Active {
            // sifei: get_action  (mint/burn/whitelist/blacklist/config/SetRoles)
            let action = proposal.content.action;

            // GB: if the caller_role does not match the role required by the action, then return error.
            // GBTODO: the voter can not vote twice.
            if let Some(role) = Self::get_voter_with_action(action) {
                if caller_role != role {
                    return Err(Error::InvalidRole);
                }
            } else {
                return Err(Error::InvalidRole);
            }


            // sifei: define get_quorum from state with action for the following usage.
            let quorum = Self::get_quorum(ctx.runtime_state(), action)?;
            if quorum > 100 {
                return Err(Error::InvalidQuorum);
            }


            // Sifei: get total no of voters from role based on action
            let voter_total:u16 = Self::get_voters_num_with_action(ctx.runtime_state(), action)?;
            // sifei: if the vote_count exceed the requirements of specific action (mint), 
            let vote_count = proposal.add_vote(body.option);
            if body.option == Vote::VoteYes {
                // GB: round up to ensure enough votes.
                let result = voter_total as u32 * quorum as u32;
                let threshold = (result + 99) / 100; // +99 is equivalent to + (divisor - 1)

                if  vote_count  >= (threshold as u16)  {
                    // this is the interface for invoke action mint/burn/whitelist/blacklist/config function.
                    let proposaldata = proposal.content.data.clone();
                    match action {
                        Action::Mint =>  {
                            //get data from proposalData and invoke mint
                            let mintaddress = match proposaldata.address {
                                None  =>  return Err(Error::NotFound),
                                Some(addr) => addr,
                            };
                            let mintamount  = match proposaldata.amount {
                                None =>  return Err(Error::NotFound),
                                Some(amt) => amt,
                            };
                            Self::mint(ctx, mintaddress, &mintamount)?;
                        },
                        Action::Burn => {
                            //get data from proposalData and invoke burn
                            let burnaddress = match proposaldata.address {
                                None  =>  return Err(Error::NotFound),
                                Some(addr) => addr,
                            };
                            let burnamount  = match proposaldata.amount {
                                None =>  return Err(Error::NotFound),
                                Some(amt) => amt,
                            };
                            Self::burn(ctx, burnaddress, &burnamount)?;
                        },
                        Action::Whitelist =>  {
                            //get data from proposalData and invoke Whitelist
                            let whitelistaddress = match proposaldata.address {
                                None  =>  return Err(Error::NotFound),
                                Some(addr) => addr,
                            };

                            //set current role for account
                            Self::set_role(ctx.runtime_state(), whitelistaddress, Role::WhitelistedUser);
                            // Self::add_address_to_roles(ctx.runtime_state(), whitelistaddress, Role::WhitelistedUser)?;
                            //set whitelist role for account
                            Self::add_role_to_address(ctx.runtime_state(), whitelistaddress, Role::WhitelistedUser);

                        },
                        Action::Blacklist =>  {
                            //get data from proposalData and invoke Blacklist
                            let blacklistaddress = match proposaldata.address {
                                None  =>  return Err(Error::NotFound),
                                Some(addr) => addr,
                            };

                            //set role for account
                            Self::set_role(ctx.runtime_state(), blacklistaddress, Role::BlacklistedUser);
                            //set blacklist role for account
                            Self::add_role_to_address(ctx.runtime_state(), blacklistaddress, Role::BlacklistedUser);
                        },

                        Action::Config => {
                            //get data from proposalData and invoke config
                            if proposaldata.mint_quorum != None {
                                Self::set_quorum(ctx.runtime_state(), Action::Mint,proposaldata.mint_quorum.unwrap())?;
                            }
                            if proposaldata.burn_quorum != None {
                                Self::set_quorum(ctx.runtime_state(), Action::Burn,proposaldata.burn_quorum.unwrap())?;
                            }
                            if proposaldata.whitelist_quorum != None {
                                Self::set_quorum(ctx.runtime_state(), Action::Whitelist,proposaldata.whitelist_quorum.unwrap())?;
                            }
                            if proposaldata.blacklist_quorum != None {
                                Self::set_quorum(ctx.runtime_state(), Action::Blacklist,proposaldata.blacklist_quorum.unwrap())?;
                            }
                            if proposaldata.config_quorum != None {
                                Self::set_quorum(ctx.runtime_state(), Action::Config,proposaldata.config_quorum.unwrap())?;
                            }

                        },
                        Action::NoAction => {
                            // no actions
                        },
                        Action::SetRoles => {
                            //get data from proposalData and SetRoles
                            let editroleaddress = match proposaldata.address {
                                None  =>  return Err(Error::NotFound),
                                Some(addr) => addr,
                            };
                            let editrolerole  = match proposaldata.role {
                                None =>  return Err(Error::NotFound),
                                Some(rl) => rl,
                            };
                            //set current role for account
                            Self::set_role(ctx.runtime_state(), editroleaddress, editrolerole);
                            //set editrole role for account
                            Self::add_role_to_address(ctx.runtime_state(), editroleaddress, editrolerole);
                        },
                    }
                    // then change the proposal state and clear the voteOption to save space.
                    proposal.state = ProposalState::Passed;
                    proposal.voteOption = None;
                }

                //saved proposal late
            } else if  body.option == Vote::VoteNo {
                // GB: round up to ensure enough votes.
                let result = voter_total as u32 * (100 - quorum) as u32;
                let threshold = (result + 99) / 100; // +99 is equivalent to + (divisor - 1)

                if  vote_count  >= (threshold as u16)  {
                    // then change the proposal state.
                    proposal.state = ProposalState::Rejected;
                    proposal.voteOption = None;
                }
            } else {
                // proposal cancelled if half of voters abstain.
                // GBTODO: further verify and refine later.
                if vote_count as f32 >= voter_total as f32 * 0.5 {                    
                    proposal.state = ProposalState::Cancelled;
                    proposal.voteOption = None;
                }
            }

            // finally, save the updated proposal.
            Self::insert_proposal(ctx.runtime_state(), proposal)?;
        }else{
            return Err(Error::InvalidState);
        }

        Ok(())
    }


    #[handler(prefetch = "accounts.InitOwners")]
    fn prefetch_initowners(
        add_prefix: &mut dyn FnMut(Prefix),
        body: cbor::Value,
        auth_info: &AuthInfo,
    ) -> Result<(), crate::error::RuntimeError> {
        let from = auth_info.signer_info[0].address_spec.address();
        let args: Vec<types::RoleAddress> = cbor::from_value(body).map_err(|_| Error::InvalidArgument)?;

        // Prefetch accounts 'from'.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, from.as_ref()].concat(),
        ));

        // Prefetch accounts roles to be defined.
        for role_address in args.iter() {
            add_prefix(Prefix::from(
                [MODULE_NAME.as_bytes(), state::ACCOUNTS, role_address.address.as_ref()].concat(),
            ));
        }

        Ok(())
    }

    #[handler(call = "accounts.InitOwners")]
    fn tx_initowners<C: TxContext>(ctx: &mut C, body: Vec<types::RoleAddress>) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());
        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, params.gas_costs.tx_managest)?;

        if ctx.tx_caller_address() == params.chain_initiator {
            let initiator_status: bool = Self::get_initstatus(ctx.runtime_state(), params.chain_initiator)?;
            if !initiator_status {
                // GB: set init to be true, and the set_owners can only be called once.
                Self::set_initstatus(ctx.runtime_state(), params.chain_initiator, true);

                for role_address in body.iter() {
                    // GB: set the new role for the accounts in body.
                    Self::set_role(ctx.runtime_state(), role_address.address, role_address.role);

                    // oasis12389xa... minter
                    // key:minter ==> value: vec{oasis12389xa, oasis12389xb, oasis12389xc}
                    Self::add_role_to_address(ctx.runtime_state(), role_address.address, role_address.role);
                }
            }

        }else{
            return Err(Error::Forbidden);            
        }

        Ok(())
    }


    // GB: insert mintst/burnst API for tx invoked. Disable this part later to mint by proposal only.
    #[handler(prefetch = "accounts.MintST")]
    fn prefetch_mintst(
        add_prefix: &mut dyn FnMut(Prefix),
        body: cbor::Value,
        auth_info: &AuthInfo,
    ) -> Result<(), crate::error::RuntimeError> {
        let args: types::MintST = cbor::from_value(body).map_err(|_| Error::InvalidArgument)?;
        // GB: this address should be NetworkInitiator.
        let from = auth_info.signer_info[0].address_spec.address();

        // Prefetch accounts 'to'.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, args.to.as_ref()].concat(),
        ));
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::BALANCES, args.to.as_ref()].concat(),
        ));
        // GB: this from account should be meaningless for mintst txs.
        // Prefetch accounts 'from'.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, from.as_ref()].concat(),
        ));

        Ok(())
    }

    #[handler(call = "accounts.MintST")]
    fn tx_mintst<C: TxContext>(ctx: &mut C, body: types::MintST) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());

        // GBTODO: insert params.mint_disabled similar as transfers_disabled.
        // GBDONE: refer line 103 this file.
        // Reject mints when they are disabled.
        if params.mintst_disabled {
            return Err(Error::Forbidden);
        }

        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, params.gas_costs.tx_managest)?;


        // GB: call the mint function directly.
        Self::mint(ctx, body.to, &body.amount)?;

        Ok(())
    }



    #[handler(prefetch = "accounts.BurnST")]
    fn prefetch_burnst(
        add_prefix: &mut dyn FnMut(Prefix),
        _body: cbor::Value,
        auth_info: &AuthInfo,
    ) -> Result<(), crate::error::RuntimeError> {
        // GB: this address should be NetworkInitiator.
        let from = auth_info.signer_info[0].address_spec.address();

        // Prefetch accounts 'from'.
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::ACCOUNTS, from.as_ref()].concat(),
        ));
        add_prefix(Prefix::from(
            [MODULE_NAME.as_bytes(), state::BALANCES, from.as_ref()].concat(),
        ));

        Ok(())
    }

    // GB: insert burn API for tx invoked.
    // GBTODO: insert types::Burn for parameters of tx_burn.
    // GBDONE: refer to /accounts/types.rs:22.
    #[handler(call = "accounts.BurnST")]
    fn tx_burnst<C: TxContext>(ctx: &mut C, body: types::BurnST) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());

        // Reject burnst when they are disabled.
        if params.burnst_disabled {
            return Err(Error::Forbidden);
        }

        // GB: introduce new parameter field chain_initiator.
        if ctx.tx_caller_address() != params.chain_initiator {
            return Err(Error::Forbidden);
        }

        <C::Runtime as Runtime>::Core::use_tx_gas(ctx, params.gas_costs.tx_managest)?;

        // GB: call the burn function directly.
        Self::burn(ctx, ctx.tx_caller_address(), &body.amount)?;

        Ok(())
    }



    // GB: insert for info query.
    #[handler(query = "accounts.Role")]
    fn query_role<C: Context>(ctx: &mut C, args: types::RoleQuery) -> Result<role::Role, Error> {
        Self::get_role(ctx.runtime_state(), args.address)
    }

    #[handler(query = "accounts.Init")]
    fn query_init<C: Context>(ctx: &mut C, args: types::InitInfoQuery) -> Result<bool, Error> {
        Self::get_initstatus(ctx.runtime_state(), args.address)
    }


    #[handler(query = "accounts.Quorum")]
    fn query_quorum<C: Context>(ctx: &mut C, args: types::QuorumQuery) -> Result<u8, Error> {
        Self::get_quorum(ctx.runtime_state(), args.action)
    }


    #[handler(query = "accounts.RoleAddresses", expensive)]
    fn query_roleaddresses<C: Context>(
        ctx: &mut C,
        args: types::RoleAddressesQuery,
    ) -> Result<Vec<Address>, Error> {
        Self::get_addresses_in_role(ctx.runtime_state(), args.role)
    }


    #[handler(query = "accounts.ProposalID")]
    fn query_proposal_id<C: Context>(ctx: &mut C, _dummy: ()) -> Result<u32, Error> {
        Self::get_proposal_id(ctx.runtime_state())
    }

    #[handler(query = "accounts.ProposalInfo")]
    fn query_proposal<C: Context>(ctx: &mut C, id: u32) -> Result<types::Proposal, Error> {
        Self::get_proposal(ctx.runtime_state(), id)
    }

/*####################################################################################################*/



    #[handler(query = "accounts.Nonce")]
    fn query_nonce<C: Context>(ctx: &mut C, args: types::NonceQuery) -> Result<u64, Error> {
        Self::get_nonce(ctx.runtime_state(), args.address)
    }

    #[handler(query = "accounts.Addresses", expensive)]
    fn query_addresses<C: Context>(
        ctx: &mut C,
        args: types::AddressesQuery,
    ) -> Result<Vec<Address>, Error> {
        Self::get_addresses(ctx.runtime_state(), args.denomination)
    }

    #[handler(query = "accounts.Balances")]
    fn query_balances<C: Context>(
        ctx: &mut C,
        args: types::BalancesQuery,
    ) -> Result<types::AccountBalances, Error> {
        Self::get_balances(ctx.runtime_state(), args.address)
    }

    #[handler(query = "accounts.DenominationInfo")]
    fn query_denomination_info<C: Context>(
        ctx: &mut C,
        args: types::DenominationInfoQuery,
    ) -> Result<types::DenominationInfo, Error> {
        Self::get_denomination_info(ctx.runtime_state(), &args.denomination)
    }
}

impl module::Module for Module {
    const NAME: &'static str = MODULE_NAME;
    type Error = Error;
    type Event = Event;
    type Parameters = Parameters;
}

impl Module {
    /// Initialize state from genesis.
    pub fn init<C: Context>(ctx: &mut C, genesis: Genesis) {
        // Create accounts.
        let mut store = storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let mut accounts =
            storage::TypedStore::new(storage::PrefixStore::new(&mut store, &state::ACCOUNTS));
        for (address, account) in genesis.accounts {
            accounts.insert(address, account);
        }

        // Create balances.
        let mut balances = storage::PrefixStore::new(&mut store, &state::BALANCES);
        let mut computed_total_supply: BTreeMap<token::Denomination, u128> = BTreeMap::new();
        for (address, denominations) in genesis.balances.iter() {
            let mut account =
                storage::TypedStore::new(storage::PrefixStore::new(&mut balances, &address));
            for (denomination, value) in denominations {
                account.insert(denomination, value);

                // Update computed total supply.
                computed_total_supply
                    .entry(denomination.clone())
                    .and_modify(|v| *v += value)
                    .or_insert_with(|| *value);
            }
        }

        // Validate and set total supply.
        let mut total_supplies =
            storage::TypedStore::new(storage::PrefixStore::new(&mut store, &state::TOTAL_SUPPLY));
        for (denomination, total_supply) in genesis.total_supplies.iter() {
            let computed = computed_total_supply
                .remove(denomination)
                .expect("unexpected total supply");
            assert!(
                &computed == total_supply,
               "unexpected total supply (expected: {total_supply} got: {computed})",
            );

            total_supplies.insert(denomination, total_supply);
        }
        for (denomination, total_supply) in computed_total_supply.iter() {
            panic!("missing expected total supply: {total_supply} {denomination}",);
        }

        // Validate genesis parameters.
        genesis
            .parameters
            .validate_basic()
            .expect("invalid genesis parameters");

        // Set genesis parameters.
        Self::set_params(ctx.runtime_state(), genesis.parameters);
    }

    /// Migrate state from a previous version.
    fn migrate<C: Context>(_ctx: &mut C, _from: u32) -> bool {
        // No migrations currently supported.
        false
    }
}

impl module::MigrationHandler for Module {
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

impl module::TransactionHandler for Module {
    fn authenticate_tx<C: Context>(
        ctx: &mut C,
        tx: &Transaction,
    ) -> Result<(), modules::core::Error> {
        // println!("gbtest file: {}, line: {}", file!(), line!());
        // Check whether the transaction is currently valid.
        let round = ctx.runtime_header().round;
        if let Some(not_before) = tx.auth_info.not_before {
            if round < not_before {
                // Too early.
                return Err(modules::core::Error::ExpiredTransaction);
            }
        }
        if let Some(not_after) = tx.auth_info.not_after {
            if round > not_after {
                // Too late.
                return Err(modules::core::Error::ExpiredTransaction);
            }
        }


        // Check nonces.
        let payer = Self::check_signer_nonces(ctx, &tx.auth_info)?;

        // GB: check blacklisted user here.
        let addr_role = Self::get_role(ctx.runtime_state(), payer).unwrap_or_default();
        if addr_role == Role::BlacklistedUser {
            return Err(modules::core::Error::NotAuthenticated);
        }


        // Charge the specified amount of fees.
        if !tx.auth_info.fee.amount.amount().is_zero() {
            if ctx.is_check_only() {
                // Do not update balances during transaction checks. In case of checks, only do it
                // after all the other checks have already passed as otherwise retrying the
                // transaction will not be possible.
                Self::ensure_balance(ctx.runtime_state(), payer, &tx.auth_info.fee.amount)
                    .map_err(|_| modules::core::Error::InsufficientFeeBalance)?;
            } else {
                // Actually perform the move.
                Self::move_into_fee_accumulator(ctx, payer, &tx.auth_info.fee.amount)?;
            }

            // TODO: Emit event that fee has been paid.

            let gas_price = tx.auth_info.fee.gas_price();
            // Bump transaction priority.
            <C::Runtime as Runtime>::Core::add_priority(
                ctx,
                gas_price.try_into().unwrap_or(u64::MAX),
            )?;
        }

        // Do not update nonces early during transaction checks. In case of checks, only do it after
        // all the other checks have already passed as otherwise retrying the transaction will not
        // be possible.
        if !ctx.is_check_only() {
            Self::update_signer_nonces(ctx, &tx.auth_info)?;
        }

        Ok(())
    }

    fn after_dispatch_tx<C: Context>(
        ctx: &mut C,
        tx_auth_info: &AuthInfo,
        result: &module::CallResult,
    ) {
        if !ctx.is_check_only() {
            // Do nothing outside transaction checks.
            return;
        }
        if !matches!(result, module::CallResult::Ok(_)) {
            // Do nothing in case the call failed to allow retries.
            return;
        }

        // Update payer balance.
        let payer = Self::check_signer_nonces(ctx, tx_auth_info).unwrap(); // Already checked.
        let amount = &tx_auth_info.fee.amount;
        Self::sub_amount(ctx.runtime_state(), payer, amount).unwrap(); // Already checked.

        // Update nonces.
        Self::update_signer_nonces(ctx, tx_auth_info).unwrap();
    }
}

impl module::BlockHandler for Module {
    fn end_block<C: Context>(ctx: &mut C) {
        // Determine the fees that are available for disbursement from the last block.
        // MZ, this takes long time
        /*
        let previous_fees = Self::get_balances(ctx.runtime_state(), *ADDRESS_FEE_ACCUMULATOR)
            .expect("get_balances must succeed")
            .balances;
        */

        let previous_fee = Self::get_balance(
            ctx.runtime_state(),
            *ADDRESS_FEE_ACCUMULATOR,
            token::Denomination::NATIVE,
        )
        .expect("get_balance must succeed");

        // Drain previous fees from the fee accumulator.
        /*
        for (denom, remainder) in &previous_fees {
            Self::sub_amount(
                ctx.runtime_state(),
                *ADDRESS_FEE_ACCUMULATOR,
                &token::BaseUnits::new(*remainder, denom.clone()),
            )
            .expect("sub_amount must succeed");
        }
        */

        Self::sub_amount(
            ctx.runtime_state(),
            *ADDRESS_FEE_ACCUMULATOR,
            &token::BaseUnits::new(previous_fee, token::Denomination::NATIVE),
        )
        .expect("sub_amount must succeed");

        // Disburse transaction fees to entities controlling all the good nodes in the committee.
        let addrs: Vec<Address> = ctx
            .runtime_round_results()
            .good_compute_entities
            .iter()
            .map(|pk| Address::from_sigspec(&SignatureAddressSpec::Ed25519(pk.into())))
            .collect();

        if !addrs.is_empty() {
            // 1. Get the total amount of fees.
            // NOTE: demonination is not used here, as we assume that all fees are in the same denomination.
            /*
            let total_fees: u128 = previous_fees
                .into_values()
                .sum();
            */
            let total_fees = previous_fee;

            // 2. Tax (10% of the total fees) is transferred to the common pool.
            let tax: u128 = total_fees
                .checked_div(10)
                .expect("10% of the total fees should be non-zero");
            Self::add_amount(
                ctx.runtime_state(),
                *ADDRESS_COMMON_POOL,
                &token::BaseUnits::new(
                    tax, token::Denomination::NATIVE),
            )
            .expect("add_amount must succeed for transfer to the common pool (taxation)");

            // 3. The remaining fees are distributed among the good nodes.
            let remaining_fees = total_fees
                .checked_sub(tax)
                .expect("remaining fees should be non-zero");
            // Divide the remaining fees equally among the good nodes
            let each_node_fee = remaining_fees
                .checked_div(addrs.len() as u128)
                .expect("addrs is non-empty");

            for address in addrs {
                Self::add_amount(
                    ctx.runtime_state(), 
                    address, 
                    &token::BaseUnits::new(
                        each_node_fee, token::Denomination::NATIVE))
                .expect("add_amount must succeed for fee disbursement");
            }
        }

        // Fees for the active block should be transferred to the fee accumulator address.
        let acc = ctx
            .value::<FeeAccumulator>(CONTEXT_KEY_FEE_ACCUMULATOR)
            .take()
            .unwrap_or_default();
        for (denom, amount) in acc.total_fees.into_iter() {
            Self::add_amount(
                ctx.runtime_state(),
                *ADDRESS_FEE_ACCUMULATOR,
                &token::BaseUnits::new(amount, denom),
            )
            .expect("add_amount must succeed for transfer to fee accumulator")
        }
    }
}

impl module::InvariantHandler for Module {
    /// Check invariants.
    fn check_invariants<C: Context>(ctx: &mut C) -> Result<(), CoreError> {
        // All account balances should sum up to the total supply for their
        // corresponding denominations.

        #[allow(clippy::or_fun_call)]
        let balances = Self::get_all_balances(ctx.runtime_state()).or(Err(
            CoreError::InvariantViolation("unable to get balances of all accounts".to_string()),
        ))?;
        #[allow(clippy::or_fun_call)]
        let total_supplies = Self::get_total_supplies(ctx.runtime_state()).or(Err(
            CoreError::InvariantViolation("unable to get total supplies".to_string()),
        ))?;

        // First, compute total supplies based on account balances.
        let mut computed_ts: BTreeMap<token::Denomination, u128> = BTreeMap::new();

        for bals in balances.values() {
            for (den, amt) in bals {
                computed_ts
                    .entry(den.clone())
                    .and_modify(|a| *a += amt)
                    .or_insert_with(|| *amt);
            }
        }

        // Now check if the computed and given total supplies match.
        for (den, ts) in &total_supplies {
            // Return error if total supplies have a denomination that we
            // didn't encounter when computing total supplies based on account
            // balances.
            #[allow(clippy::or_fun_call)]
            let computed = computed_ts
                .remove(den)
                .ok_or(CoreError::InvariantViolation(
                    "unexpected denomination".to_string(),
                ))?;

            if &computed != ts {
                // Computed and actual total supplies don't match.
                return Err(CoreError::InvariantViolation(format!(
                    "computed and actual total supplies don't match (computed={computed}, actual={ts})",
                )));
            }
        }

        // There should be no remaining denominations in the computed supplies,
        // because that would mean that accounts have denominations that don't
        // appear in the total supplies table, which would obviously be wrong.
        if computed_ts.is_empty() {
            Ok(())
        } else {
            Err(CoreError::InvariantViolation(
                "encountered denomination that isn't present in total supplies table".to_string(),
            ))
        }
    }
}
