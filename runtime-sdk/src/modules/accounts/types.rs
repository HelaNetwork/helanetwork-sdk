//! Account module types.
use std::collections::{BTreeMap, HashMap};

use crate::types::{address::Address, role::Role, token, proposal, vote};


/// Transfer call.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct Transfer {
    pub to: Address,
    pub amount: token::BaseUnits,
}


// GB: insert addresses for roles.
// This variable name (address, role) must be consistent with the one defined in client-sdk.
// As they are both encoded and decoded by cbor, otherwise, invalid type is returned.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct RoleAddress {
    pub address: Address,
    pub role: Role,
}


#[derive(Clone, Debug, Default, PartialEq, cbor::Encode, cbor::Decode)]
pub struct ProposalContent {
    pub action: vote::Action,
    pub data: ProposalData,
}


#[derive(Clone, Debug, Default, PartialEq, cbor::Encode, cbor::Decode)]
pub struct ProposalData {
    #[cbor(optional)]
    pub address: Option<Address>,
    #[cbor(optional)]
    pub amount: Option<token::BaseUnits>,
    #[cbor(optional)]
    pub meta: Option<proposal::Meta>,
    #[cbor(optional)]
    pub role: Option<Role>,
    #[cbor(optional)]
    pub mint_quorum: Option<u8>,
    #[cbor(optional)]
    pub burn_quorum: Option<u8>,
    #[cbor(optional)]
    pub whitelist_quorum: Option<u8>,
    #[cbor(optional)]
    pub blacklist_quorum: Option<u8>,
    #[cbor(optional)]
    pub config_quorum: Option<u8>,
    // GB: setRoles_quorum is omit here, which means it is 100 by default.
}


// Proposal is for mint/burn/blacklist/edit_roles etc. by SNAP.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
#[allow(non_snake_case)]
pub struct Proposal {
    // ID is the unique identifier of the proposal.
    pub id: u32,
    // Submitter is the address of the proposal submitter.
    pub submitter: Address,
    // State is the state of the proposal.
    pub state: proposal::ProposalState,

    // Content is the content of the proposal.
    pub content: ProposalContent,

    // Results are the final tallied results after the voting period has ended, 
    // 2**16 = 65536 voters at most for a vote.
    pub results: Option<HashMap<vote::Vote, u16>>,

    // Record the addresses voted.
    pub voteOption: Option<HashMap<Address, vote::Vote>>,
}

impl Proposal {
    pub fn add_vote(&mut self, vote: vote::Vote) -> u16 {
        // Initialize the results HashMap if it's not initialized.
        if self.results.is_none() {
            self.results = Some(HashMap::new());
        }

        // Unwrap the Option and increment the vote count.
        let results = self.results.as_mut().unwrap();
        let count = results.entry(vote).or_insert(0);
        *count += 1;

        // Return the updated count.
        *count
    }
}


#[derive(Clone, Debug, Default, PartialEq, cbor::Encode, cbor::Decode)]
pub struct VoteProposal {
    pub id: u32,
    pub option: vote::Vote,
}


// GB: insert mintst.
// Mint call.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct MintST {
    pub to: Address,
    pub amount: token::BaseUnits,
}

// GB: insert burnst.
// Burn call.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct BurnST {
    // comment from field, as no use mostly.
    // pub from: Address,
    pub amount: token::BaseUnits,
}


/// Account metadata.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct Account {
    #[cbor(optional)]
    pub nonce: u64,

    // GBTODO: define roles (user, admin, bank) for each account in runtime-sdk/src/types mod.
    #[cbor(optional)]
    pub role: Role,

    // GB: set bool var to be true, after the chainInitiator set the in
    #[cbor(optional)]
    pub init: bool,
}


/// Arguments for the Nonce query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct NonceQuery {
    pub address: Address,
}

/// Arguments for the Role query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct RoleQuery {
    pub address: Address,
}

/// Arguments for the InitStatus query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct InitInfoQuery {
    pub address: Address,
}
/// Arguments for the Blacklist query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct BlacklistQuery {
    pub address: Address,
}

/// Arguments for the Quorum query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct QuorumQuery {
    pub action: vote::Action,
}

/// Arguments for the Role Addresses query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct RoleAddressesQuery {
    pub role: Role,
}

/// Arguments for the Addresses query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct AddressesQuery {
    pub denomination: token::Denomination,
}

/// Arguments for the Balances query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct BalancesQuery {
    pub address: Address,
}

/// Balances in an account.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct AccountBalances {
    pub balances: BTreeMap<token::Denomination, u128>,
}

/// Arguments for the DenominationInfo query.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct DenominationInfoQuery {
    pub denomination: token::Denomination,
}

/// Information about a denomination.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct DenominationInfo {
    /// Number of decimals that the denomination is using.
    pub decimals: u8,
}
