package accounts

import (
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"
)

// Transfer is the body for the accounts.Transfer call.
type Transfer struct {
	To     types.Address   `json:"to"`
	Amount types.BaseUnits `json:"amount"`
}

// GB: RoleAddress is the body for the accounts.InitOwners call.
type RoleAddress struct {
	Addr types.Address `json:"address"`
	Role types.Role    `json:"role"`
}

// GB: this Proposal type is used for user input and output.
type ProposalContent struct {
	Action types.Action       `json:"action"`
	Data   types.ProposalData `json:"data"`
}

func (pc *ProposalContent) String() (map[string]string, error) {
	result := make(map[string]string)
	result["Action"] = pc.Action.String()
	content, err := pc.Data.String(pc.Action)
	if err != nil {
		return nil, err
	}
	for key, value := range content {
		result[key] = value
	}
	return result, nil
}

// GB: this Proposal type is used for inquiries of proposals on the chain by users.
type ProposalOutput struct {
	ID        uint32
	Submitter types.Address
	State     types.ProposalState
	Content   ProposalContent
	Results   map[types.Vote]uint16
    VoteOption map[types.Address]types.Vote
}

type VoteProposal struct {
	ID     uint32     `json:"id"`
	Option types.Vote `json:"option"`
}

// GB: MintST is the body for the accounts.MintST call.
type MintST struct {
	To     types.Address   `json:"to"`
	Amount types.BaseUnits `json:"amount"`
}

type BurnST struct {
	// From     types.Address   `json:"from"`
	Amount types.BaseUnits `json:"amount"`
}

// NonceQuery are the arguments for the accounts.Nonce query.
type NonceQuery struct {
	Address types.Address `json:"address"`
}

// RoleQuery are the arguments for the accounts.Role query.
type RoleQuery struct {
	Address types.Address `json:"address"`
}

// InitInfoQuery are the arguments for the accounts.Init query.
type InitInfoQuery struct {
	Address types.Address `json:"address"`
}

// BlacklistQuery are the arguments for the accounts.Blacklisted query.
type BlacklistQuery struct {
	Address types.Address `json:"address"`
}

// RoleAddressesQuery are the arguments for the accounts.RoleAddresses query.
// GB: change Role to role, use the lower case to transfer between front and back.
type RoleAddressesQuery struct {
	Role types.Role `json:"role"`
}


type QuorumsQuery struct {
	Action types.Action `json:"action"`
}


// BalancesQuery are the arguments for the accounts.Balances query.
type BalancesQuery struct {
	Address types.Address `json:"address"`
}

// AccountBalances are the balances in an account.
type AccountBalances struct {
	Balances map[types.Denomination]types.Quantity `json:"balances"`
}

// AddressesQuery are the arguments for the accounts.Addresses query.
type AddressesQuery struct {
	Denomination types.Denomination `json:"denomination"`
}

// DenominationInfoQuery are the arguments for the accounts.DenominationInfo query.
type DenominationInfoQuery struct {
	Denomination types.Denomination `json:"denomination"`
}

// DenominationInfo represents information about a denomination.
type DenominationInfo struct {
	// Decimals is the number of decimals that the denomination is using.
	Decimals uint8 `json:"decimals"`
}

// Addresses is the response of the accounts.Addresses or accounts.RoleAddresses query.
type Addresses []types.Address

// GasCosts are the accounts module gas costs.
type GasCosts struct {
	TxTransfer uint64 `json:"tx_transfer"`
	// GB: insert fields for tx_mintst/tx_burnst.
	TxMintST uint64 `json:"tx_mintst"`
	TxBurnST uint64 `json:"tx_burnst"`

	TxInitOwners uint64 `json:"tx_initowners"`
	TxManageST   uint64 `json:"tx_managest"`
}

// Parameters are the parameters for the accounts module.
type Parameters struct {
	TransfersDisabled      bool                                    `json:"transfers_disabled"`
	MintSTDisabled         bool                                    `json:"mintst_disabled"`
	BurnSTDisabled         bool                                    `json:"burnst_disabled"`
	GasCosts               GasCosts                                `json:"gas_costs"`
	DebugDisableNonceCheck bool                                    `json:"debug_disable_nonce_check,omitempty"`
	DenominationInfos      map[types.Denomination]DenominationInfo `json:"denomination_infos,omitempty"`
}

// ModuleName is the accounts module name.
const ModuleName = "accounts"

const (
	// TransferEventCode is the event code for the transfer event.
	TransferEventCode = 1
	// BurnEventCode is the event code for the burn event.
	BurnEventCode = 2
	// MintEventCode is the event code for the mint event.
	MintEventCode = 3
)

// TransferEvent is the transfer event.
type TransferEvent struct {
	From   types.Address   `json:"from"`
	To     types.Address   `json:"to"`
	Amount types.BaseUnits `json:"amount"`
}

// BurnEvent is the burn event.
type BurnEvent struct {
	Owner  types.Address   `json:"owner"`
	Amount types.BaseUnits `json:"amount"`
}

// MintEvent is the mint event.
type MintEvent struct {
	Owner  types.Address   `json:"owner"`
	Amount types.BaseUnits `json:"amount"`
}

// GB: Event::Transfer may come from here.
// GBTODO: insert MintSTEvent.
// Event is an account event.
type Event struct {
	Transfer *TransferEvent
	Burn     *BurnEvent
	Mint     *MintEvent
}
